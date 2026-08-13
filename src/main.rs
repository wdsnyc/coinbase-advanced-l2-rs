mod cli;
mod display;
mod event;
mod feed;
mod order_book;
mod products;
mod side;
mod subscription;
mod trade;

use clap::Parser;

fn main() {
    let args = cli::Cli::parse();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default rustls CryptoProvider");

    let process_snapshots = !args.no_snapshots;
    feed::run(&args.symbol_list, &args.secrets_dir, process_snapshots);
}
