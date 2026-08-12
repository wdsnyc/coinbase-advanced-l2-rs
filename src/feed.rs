use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tungstenite::{connect, Message};

use crate::event::ChannelMessage;
use crate::order_book::{OrderBook, PriceBook, PriceMap};
use crate::products::{self, PrecisionTable};
use crate::side::Side;
use crate::subscription;
use crate::display;

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";

/// Rust equivalent of coinbase_feed::run() -- same shape as the C++
/// version's synchronous Boost.Asio read loop: connect, send the
/// subscribe message, then block on read() in a loop, one message at a
/// time, same OS thread throughout. No async runtime involved.
pub fn run(symbols: &[String], secrets_dir: &str, process_snapshots: bool) {
    // Step 6 -- precision table, built once before the book exists.
    let precision = products::build_precision_table(symbols, secrets_dir);

    let subscribe_msg = subscription::get_subscribe_msg(symbols, secrets_dir);

    let (mut socket, _response) =
        connect(WS_URL).expect("failed to connect to Coinbase WS");

    println!("connected to {WS_URL}");

    socket
        .send(Message::text(subscribe_msg))
        .expect("failed to send subscribe message");

    // The actual order book, one OrderBook per subscribed product,
    // created lazily as each product_id is first seen.
    let mut book: PriceMap = PriceMap::new();

    // Counts snapshot-type events seen (not applied) -- matches
    // orderBook.h's snapshotNum: incremented regardless of
    // process_snapshots, so the mismatch against actual book size is
    // what makes --no_snapshots's effect visible. Only ever touched
    // from this thread, so a plain u64 is enough -- no Arc/Mutex
    // needed here, unlike current_symbol/dump_enabled below.
    //
    // Expected behavior, confirmed against real testing: Coinbase sends
    // exactly one snapshot per subscribed symbol, at subscribe time --
    // not periodically. So this should climb to symbols.len() early in
    // the connection and then plateau there for the rest of the
    // session. If it climbs past that, something unusual happened (a
    // resubscribe, a reconnect, a change in Coinbase's behavior) and is
    // worth investigating rather than assuming is normal. Also means
    // --no_snapshots's effect is permanent for the whole session, not
    // just a slow start -- the book stays missing most of its levels
    // the entire time, since the one snapshot it needed never repeats.
    let mut snapshot_count: u64 = 0;

    // #2 -- shared state for the symbol-switch thread below. Mutex for
    // the symbol string (no native atomic for String), AtomicBool for
    // the dump-enabled flag -- direct match for the C++ version's
    // std::atomic<bool> m_orderBookDump, since bool does have a real
    // atomic type. Starts on the first symbol, dump enabled -- same
    // defaults as coinbase_feed's constructor.
    let current_symbol = Arc::new(Mutex::new(symbols[0].clone()));
    let dump_enabled = Arc::new(AtomicBool::new(true));

    // #3 -- mirrors readFromStdinThread(): loops forever, waits for
    // Enter, pauses the display, prompts for a symbol, and if valid
    // switches current_symbol and resumes. Detached, same as the C++
    // version's thread (never joined -- runs until the process exits).
    // symbols.to_vec() clones into an owned Vec because the closure
    // needs 'static data: the borrow checker won't let a thread::spawn
    // closure capture &[String] borrowed from run()'s stack frame,
    // since that borrow wouldn't necessarily outlive the thread -- a
    // real use-after-free in C++ if you got the lifetime wrong, a
    // compile error here instead.
    {
        let symbols = symbols.to_vec();
        let current_symbol = Arc::clone(&current_symbol);
        let dump_enabled = Arc::clone(&dump_enabled);

        std::thread::spawn(move || loop {
            println!("\nPress ENTER to change symbol...");
            let mut line = String::new();
            io::stdin()
                .read_line(&mut line)
                .expect("failed to read stdin");

            dump_enabled.store(false, Ordering::Release);

            println!("Valid symbols: {}", symbols.join(","));
            print!("Enter symbol: ");
            io::stdout().flush().expect("failed to flush stdout");

            let mut symbol = String::new();
            io::stdin()
                .read_line(&mut symbol)
                .expect("failed to read stdin");
            // read_line keeps the trailing newline -- unlike C++'s
            // std::getline, which strips it. Comparing against symbols
            // without trimming would never match.
            let symbol = symbol.trim();

            if symbols.iter().any(|s| s == symbol) {
                *current_symbol.lock().unwrap() = symbol.to_string();
                dump_enabled.store(true, Ordering::Release);
            } else {
                println!("Invalid symbol. Press ENTER to retry.");
            }
        });
    }

    loop {
        let msg = match socket.read() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("websocket error: {e}");
                break;
            }
        };

        match msg {
            Message::Text(text) => handle_message(
                text.as_str(),
                &mut book,
                &precision,
                process_snapshots,
                &current_symbol,
                &dump_enabled,
                &mut snapshot_count,
            ),
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(frame) => {
                eprintln!("connection closed: {frame:?}");
                break;
            }
            _ => continue,
        }
    }
}

/// Parses one raw text frame and, for l2_data messages, applies each
/// PriceLevelUpdate into the right symbol's PriceBook. Falls back to
/// printing the raw value for the bare {"type": ...} messages (errors,
/// acks) that don't have a "channel" field at all.
///
/// When process_snapshots is false, snapshot-type events are skipped
/// entirely -- only incremental "update" events get applied. Worth
/// knowing: with this off, the book never receives its initial full
/// state, only the changes since connection -- useful for keeping
/// terminal output down while testing update handling, not something
/// you'd want for a book that needs to reflect the real current market.
fn handle_message(
    text: &str,
    book: &mut PriceMap,
    precision: &PrecisionTable,
    process_snapshots: bool,
    current_symbol: &Arc<Mutex<String>>,
    dump_enabled: &Arc<AtomicBool>,
    snapshot_count: &mut u64,
) {
    let raw: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("failed to parse message as JSON: {e}\n{text}");
            return;
        }
    };

    // Read once, up front: while paused (waiting on the symbol-switch
    // prompt), the per-event status prints below need to respect this
    // too, not just the dump()/top_of_book() calls -- otherwise they
    // keep scrolling and bury the "Press ENTER"/"Enter symbol:" prompts
    // from the other thread. This is what looked like dump_enabled
    // "never going false": the flag was flipping correctly, these
    // prints just weren't checking it.
    let dumping = dump_enabled.load(Ordering::Acquire);

    match serde_json::from_value::<ChannelMessage>(raw.clone()) {
        Ok(ChannelMessage::L2Data { events, .. }) => {
            for event in events {
                if event.event_type == "snapshot" {
                    *snapshot_count += 1;
                }

                if event.event_type == "snapshot" && !process_snapshots {
                    continue;
                }

                let product = match precision.get(&event.product_id) {
                    Some(p) => p,
                    None => {
                        eprintln!(
                            "no precision info for {} -- skipping {} update(s)",
                            event.product_id,
                            event.updates.len()
                        );
                        continue;
                    }
                };

                let order_book = book.entry(event.product_id.clone()).or_insert_with(|| {
                    OrderBook::new(event.product_id.clone(), PriceBook::new(), PriceBook::new())
                });

                for u in &event.updates {
                    let price_ticks = product.price_to_ticks(&u.price_level);
                    let qty_ticks = product.size_to_ticks(&u.new_quantity);

                    match u.side {
                        Side::Bid => order_book.bids.book_add(price_ticks, qty_ticks),
                        Side::Offer => order_book.asks.book_add(price_ticks, qty_ticks),
                    }
                }
            }
        }
        Ok(ChannelMessage::Subscriptions { events, .. }) => {
            for event in events {
                println!("[subscriptions] level2: {:?}", event.subscriptions.level2);
            }
        }
        Err(_) if raw.get("type").is_some() => {
            println!("[type-only message] {raw}");
            return; // C++ checks dump only inside the "channel" branch -- skip it here
        }
        Err(e) => {
            eprintln!("unrecognized message shape: {e}\n{raw}");
            return;
        }
    }

    // Matches where the C++ version checks m_orderBookDump -- after
    // processing a channel-bearing message, for both l2_data and
    // subscriptions.
    if dumping {
        let symbol = current_symbol.lock().unwrap().clone();

        let order_book = match book.get(&symbol) {
            Some(ob) => ob,
            None => return, // no data for this symbol yet
        };
        let product = match precision.get(&symbol) {
            Some(p) => p,
            None => return, // shouldn't happen -- symbol came from the validated list
        };

        display::dump(order_book, product, 10);
        println!();
        display::print_stats(order_book, *snapshot_count);
        println!();
        display::top_of_book(order_book, product);
    }
}
