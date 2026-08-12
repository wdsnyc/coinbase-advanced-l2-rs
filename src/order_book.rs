use std::collections::HashMap;

use std::collections::BTreeMap;

// Price ticks -> quantity ticks, both as i64. Direct port of
// orderBook.h's book<double,double>, but keyed on integer ticks
// (via ProductInfo::price_to_ticks/size_to_ticks in products.rs)
// rather than raw floats -- see the quote_increment discussion.
#[derive(Debug, Default)]
pub struct PriceBook {
    levels: BTreeMap<i64, i64>,
}

impl PriceBook {
    pub fn new() -> Self {
        PriceBook::default()
    }

    /// Direct port of orderBook.h's bookAdd -- qty_ticks of 0 removes
    /// the level, otherwise insert-or-overwrite. insert()/remove()
    /// already give us "insert or overwrite" and "remove if present"
    /// for free, so this collapses the C++ find-then-branch into two
    /// arms.
    pub fn book_add(&mut self, price_ticks: i64, qty_ticks: i64) {
        if qty_ticks == 0 {
            self.levels.remove(&price_ticks);
        } else {
            self.levels.insert(price_ticks, qty_ticks);
        }
    }

    /// Best num_levels bids, best (highest price) first. BTreeMap is
    /// always ascending, so .rev() is what gives us highest-first --
    /// the equivalent of orderBook.h's std::greater<double> bid map,
    /// just achieved by reversing the iterator rather than choosing a
    /// different map type. Returns owned (i64, i64) pairs, not
    /// references, so callers can copy out what they need and drop
    /// whatever lock they're holding before doing anything else with
    /// the result -- same "don't hold the guard longer than needed"
    /// pattern as arc_mutex_test.
    pub fn get_bids(&self, num_levels: usize) -> Vec<(i64, i64)> {
        let mut bids = vec![];
        for (key, value) in self.levels.iter().rev().take(num_levels) {
            bids.push((*key, *value));
        }
        bids
    }

    /// Best num_levels asks, best (lowest price) first. Plain .iter()
    /// is already ascending, so no reversal needed here.
    pub fn get_asks(&self, num_levels: usize) -> Vec<(i64, i64)> {
        let mut asks = vec![];
        for (key, value) in self.levels.iter().take(num_levels) {
            asks.push((*key, *value));
        }
        asks
    }

    /// Number of price levels currently in this side of the book --
    /// used to show how much the book actually contains, e.g. to make
    /// the effect of --no_snapshots visible (snapshot count keeps
    /// climbing, but level count stays tiny without them applied).
    pub fn len(&self) -> usize {
        self.levels.len()
    }
}

#[derive(Debug)]
pub struct OrderBook {
    #[allow(dead_code)]
    pub symbol: String,
    pub bids: PriceBook,
    pub asks: PriceBook,
}

impl OrderBook {
    pub fn new(symbol: String, bids: PriceBook, asks: PriceBook) -> Self {
        OrderBook { symbol, bids, asks }
    }
}

// A type alias, not a wrapper struct -- PriceMap::new() below resolves
// straight through to HashMap::new().
pub type PriceMap = HashMap<String, OrderBook>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_add_inserts_new_level() {
        let mut book = PriceBook::new();
        book.book_add(100, 5);
        assert_eq!(book.len(), 1);
        assert_eq!(book.get_bids(10), vec![(100, 5)]);
    }

    #[test]
    fn book_add_overwrites_existing_level() {
        let mut book = PriceBook::new();
        book.book_add(100, 5);
        book.book_add(100, 9);
        assert_eq!(book.len(), 1);
        assert_eq!(book.get_bids(10), vec![(100, 9)]);
    }

    #[test]
    fn book_add_zero_qty_removes_level() {
        let mut book = PriceBook::new();
        book.book_add(100, 5);
        book.book_add(100, 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn book_add_zero_qty_on_missing_level_is_noop() {
        let mut book = PriceBook::new();
        book.book_add(100, 0);
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn get_bids_returns_highest_first() {
        let mut book = PriceBook::new();
        book.book_add(100, 1);
        book.book_add(300, 3);
        book.book_add(200, 2);
        assert_eq!(book.get_bids(10), vec![(300, 3), (200, 2), (100, 1)]);
    }

    #[test]
    fn get_asks_returns_lowest_first() {
        let mut book = PriceBook::new();
        book.book_add(300, 3);
        book.book_add(100, 1);
        book.book_add(200, 2);
        assert_eq!(book.get_asks(10), vec![(100, 1), (200, 2), (300, 3)]);
    }

    #[test]
    fn get_bids_respects_num_levels() {
        let mut book = PriceBook::new();
        book.book_add(100, 1);
        book.book_add(200, 2);
        book.book_add(300, 3);
        assert_eq!(book.get_bids(2), vec![(300, 3), (200, 2)]);
    }
}
