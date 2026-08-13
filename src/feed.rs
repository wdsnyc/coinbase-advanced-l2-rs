use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tungstenite::{connect, Message};

use crate::display;
use crate::event::ChannelMessage;
use crate::order_book::{OrderBook, PriceBook, PriceMap};
use crate::products::{self, PrecisionTable};
use crate::side::Side;
use crate::subscription;

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";

/// Rust equivalent of C++ coinbase_feed::run() which has a
/// synchronous Boost.Asio read loop: connect, send the subscribe
/// message, then block on read() in a loop, one message at a time,
/// same OS thread throughout. No async runtime involved.
pub fn run(symbols: &[String], secrets_dir: &str, process_snapshots: bool) {
    // Step 6 -- precision table, built once before the book exists.
    let precision = products::build_precision_table(symbols, secrets_dir);

    let subscribe_msg = subscription::get_subscribe_msg(symbols, secrets_dir);

    let (mut socket, _response) = connect(WS_URL).expect("failed to connect to Coinbase WS");

    println!("connected to {WS_URL}");

    socket
        .send(Message::text(subscribe_msg))
        .expect("failed to send subscribe message");

    // The actual order book, one OrderBook per subscribed product,
    // created lazily as each product_id is first seen.
    let mut book: PriceMap = PriceMap::new();

    // Counts snapshot-type events seen (not applied). Coinbase sends
    // exactly one snapshot per subscribed symbol, not periodically.
    let mut snapshot_count: u64 = 0;

    // shared state for the symbol-switch thread below. Mutex for
    // the symbol string, AtomicBool for the dump-enabled flag
    let current_symbol = Arc::new(Mutex::new(symbols[0].clone()));
    let dump_enabled = Arc::new(AtomicBool::new(true));

    // readFromStdinThread(): loops forever. Enter, pauses the
    // display, prompts for a symbol, and if valid switches
    // current_symbol and resumes. Thread (never joined -- runs until
    // the process exits).
    {
        let symbols = symbols.to_vec(); // symbols &{String] func param is only valid in
                                        // this functions body. to_vec() clones &[String]
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
            // std::getline, which strips it.
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
/// entirely -- only incremental "update" events get applied
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
