use crate::order_book::OrderBook;
use crate::products::ProductInfo;

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[1;1H";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

/// Formats a value to a specific number of decimal places.
/// Precision comes from the positional argument `decimals`
fn fmt_num(value: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, value)
}

/// Snapshot-seen count alongside current book size on each side
pub fn print_stats(order_book: &OrderBook, snapshot_count: u64) {
    println!(
        "Snapshots seen: {snapshot_count}   Bid levels: {}   Ask levels: {}",
        order_book.bids.len(),
        order_book.asks.len()
    );
}

/// Clears screen and redraws num_levels of each side, arranged the
/// way Coinbase's Advanced Trade Spot UI displays the order book
/// view:
///    - asks descending from worst to best at the top
///    - spread line
///    - bids descending from best to worst below
pub fn dump(order_book: &OrderBook, product: &ProductInfo, num_levels: usize) {
    let asks = order_book.asks.get_asks(num_levels); // best (lowest) first
    let bids = order_book.bids.get_bids(num_levels); // best (highest) first

    if asks.is_empty() || bids.is_empty() {
        return; // matches orderBook.h's early return -- nothing to show yet
    }

    print!("{CLEAR_SCREEN}");
    println!("************** ORDER BOOK ***************");
    print_header(product);

    // get_asks() is best-first (ascending)
    for (price_ticks, qty_ticks) in asks.iter().rev() {
        print_row(product, *price_ticks, *qty_ticks, RED);
    }

    print_spread(product, asks[0].0, bids[0].0);

    // get_bids() is best-first (descending)
    for (price_ticks, qty_ticks) in &bids {
        print_row(product, *price_ticks, *qty_ticks, GREEN);
    }

    println!("*****************************************");
}

/// Best price on each side plus the spread
pub fn top_of_book(order_book: &OrderBook, product: &ProductInfo) {
    let best_ask = match order_book.asks.get_asks(1).into_iter().next() {
        Some(pair) => pair,
        None => return,
    };
    let best_bid = match order_book.bids.get_bids(1).into_iter().next() {
        Some(pair) => pair,
        None => return,
    };

    println!("************** TOP OF BOOK **************");
    print_header(product);

    print_row(product, best_ask.0, best_ask.1, RED);
    print_spread(product, best_ask.0, best_bid.0);
    print_row(product, best_bid.0, best_bid.1, GREEN);

    println!("*****************************************");
}

fn print_row(product: &ProductInfo, price_ticks: i64, qty_ticks: i64, color: &str) {
    let price = fmt_num(
        product.ticks_to_price(price_ticks),
        product.price_decimal_places(),
    );
    let qty = fmt_num(
        product.ticks_to_size(qty_ticks),
        product.size_decimal_places(),
    );
    println!("{color}{price:>12} {qty:>16}{RESET}");
}

fn print_header(product: &ProductInfo) {
    let (base, quote) = product.base_and_quote_currency();
    println!(
        "{:>12} {:>16}",
        format!("price ({quote})"),
        format!("qty ({base})")
    );
    println!("{:->12} {:->16}", "", "");
}

fn print_spread(product: &ProductInfo, ask_ticks: i64, bid_ticks: i64) {
    let spread = product.ticks_to_price(ask_ticks) - product.ticks_to_price(bid_ticks);
    let spread = fmt_num(spread, product.price_decimal_places());
    println!("{:>12}  Spread {spread}", "");
}
