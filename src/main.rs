mod cli;
mod display;
mod event;
mod feed;
mod order_book;
mod products;
mod side;
mod subscription;
mod trade;

use std::sync::{Arc, Mutex};
use std::thread;

use clap::Parser;

use crate::order_book::OrderBook;
use crate::order_book::PriceBook;

fn arc_mutex_test() {
    //====================================================================
    let price_map = Arc::new(Mutex::new(order_book::PriceMap::new()));

    // "cloning" an Arc doesn't clone the PriceMap — it just bumps the ref count
    let price_map_clone = Arc::clone(&price_map);

    // to actually touch the PriceMap, you must lock it:
    {
        let mut guard = match price_map.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                eprintln!("warning: price_map lock was poisoned, recovering anyway");
                poisoned.into_inner()
            }
        };

        // guard derefs to &mut PriceMap here
        guard.insert(
            String::from("TSLA"),
            order_book::OrderBook::new(
                String::from("TSLA"),
                order_book::PriceBook::new(),
                order_book::PriceBook::new(),
            ),
        );
    } // lock released here, when `guard` goes out of scope

    let guard2 = price_map_clone.lock().unwrap();
    println!("{:?}", guard2.get("TSLA"));
}

fn arc_mutex_test_with_thread() {
    let price_map = Arc::new(Mutex::new(order_book::PriceMap::new()));
    let price_map_clone = Arc::clone(&price_map);

    let handle = thread::spawn(move || {
        let sym_tsla = String::from("TSLA");
        let tsla_bid_book = PriceBook::new();
        let tsla_ask_book = PriceBook::new();
        let tsla_order_book = OrderBook::new(sym_tsla.clone(), tsla_bid_book, tsla_ask_book);
        let mut map = price_map_clone.lock().unwrap();
        map.insert(sym_tsla.clone(), tsla_order_book);
    });

    handle.join().unwrap();
    println!("{:?}", price_map.lock().unwrap().get("TSLA"));
}

fn main() {
    let args = cli::Cli::parse();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default rustls CryptoProvider");

    arc_mutex_test();
    arc_mutex_test_with_thread();

    // Step 4 of the quote_increment work -- prove parsing works.
    let product = products::fetch_product(&args.symbol_list[0], &args.secrets_dir);
    println!("{product:?}");

    // Step 5 -- prove increment -> ticks conversion looks sane, using a
    // real (price, qty) pair pulled from an earlier snapshot: "Bid
    // BTC-USD @ 63962.27 qty 0.01874522".
    let price_ticks = product.price_to_ticks("63962.27");
    let qty_ticks = product.size_to_ticks("0.01874522");
    println!("price_ticks = {price_ticks}, qty_ticks = {qty_ticks}");

    let process_snapshots = !args.no_snapshots;
    feed::run(&args.symbol_list, &args.secrets_dir, process_snapshots);
}
