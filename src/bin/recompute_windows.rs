use clap::{Parser, ValueEnum};
use enphase_bridge::trueup::recompute;

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Typed,
    Raw,
}

#[derive(Debug, Parser)]
#[command(about = "Manually recompute energy_window wh_* fields from stored boundary_snapshots")]
struct Args {
    #[arg(long, value_enum, default_value = "typed")]
    mode: Mode,
    #[arg(long)]
    from: Option<i64>,
    #[arg(long)]
    to: Option<i64>,
    #[arg(long, default_value = "false")]
    dry_run: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let config = enphase_bridge::config::Config::load().expect("failed to load config");

    let pool = enphase_bridge::storage::db::connect(&config.storage.db_path)
        .await
        .expect("failed to open database");

    match args.mode {
        Mode::Typed => recompute::run_typed(&pool, args.from, args.to, args.dry_run).await,
        Mode::Raw => recompute::run_raw(&pool, args.from, args.to, args.dry_run).await,
    }

    pool.close().await;
}
