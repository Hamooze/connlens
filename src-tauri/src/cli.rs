use crate::descriptors;
use crate::models::{ConnectionStatus, ErrorPayload, SnapshotConnection};
use crate::registry;
use crate::scan;
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Parser)]
#[command(
    name = "connlens",
    version,
    about = "Inspect local developer app connections"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        rescan: bool,
    },
    Status,
    Providers {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvelope {
    schema_version: u8,
    connections: Vec<SnapshotConnection>,
}

pub fn maybe_run(args: Vec<String>) -> Option<i32> {
    let first = args.get(1)?;
    if !matches!(first.as_str(), "list" | "status" | "providers") {
        return None;
    }
    Some(run(args))
}

fn run(args: Vec<String>) -> i32 {
    match Cli::try_parse_from(args) {
        Ok(cli) => match execute(cli) {
            Ok(()) => 0,
            Err((code, message)) => {
                eprintln!("{message}");
                code
            }
        },
        Err(err) => {
            let _ = err.print();
            1
        }
    }
}

fn execute(cli: Cli) -> Result<(), (i32, String)> {
    match cli.command {
        Commands::List {
            json,
            provider,
            all,
            rescan,
        } => {
            let mut snapshot = if rescan {
                scan::scan_and_persist(provider.clone()).map_err(|err| (2, format_error(err)))?
            } else {
                registry::load_snapshot().map_err(|err| (2, err.to_string()))?
            };
            snapshot.connections.retain(|connection| {
                let provider_match = provider
                    .as_deref()
                    .map(|provider| connection.connection.provider == provider)
                    .unwrap_or(true);
                let visibility_match =
                    all || connection.connection.status != ConnectionStatus::Missing;
                provider_match && visibility_match
            });
            if json {
                let envelope = ListEnvelope {
                    schema_version: 1,
                    connections: snapshot.connections,
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&envelope).map_err(|err| (2, err.to_string()))?
                );
            } else {
                print_table(&snapshot.connections);
            }
        }
        Commands::Status => {
            let snapshot = registry::load_snapshot().map_err(|err| (2, err.to_string()))?;
            let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
            for connection in &snapshot.connections {
                let key = match connection.connection.status {
                    ConnectionStatus::Active => "active",
                    ConnectionStatus::Changed => "changed",
                    ConnectionStatus::Missing => "missing",
                    ConnectionStatus::Unverified => "unverified",
                };
                *counts.entry(key).or_default() += 1;
            }
            println!("running=no");
            println!("watcher_health={:?}", snapshot.watcher_health);
            println!(
                "last_scan={}",
                snapshot.last_scan.unwrap_or_else(|| "never".to_string())
            );
            for (status, count) in counts {
                println!("{status}={count}");
            }
        }
        Commands::Providers { json } => {
            let providers = scan::provider_catalog();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&providers).map_err(|err| (2, err.to_string()))?
                );
            } else {
                for provider in descriptors::bundled_descriptors() {
                    println!("{}\t{}", provider.id, provider.name);
                }
            }
        }
    }
    Ok(())
}

fn format_error(err: ErrorPayload) -> String {
    match err.detail {
        Some(detail) => format!("{}: {detail}", err.message),
        None => err.message,
    }
}

fn print_table(connections: &[SnapshotConnection]) {
    println!(
        "{:<14} {:<24} {:<24} {:<12}",
        "PROVIDER", "LABEL", "HOST/SCOPE", "STATUS"
    );
    for row in connections {
        let connection = &row.connection;
        let host_scope = connection
            .identity
            .scope
            .as_ref()
            .or(connection.identity.host.as_ref())
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<14} {:<24} {:<24} {:?}",
            connection.provider, connection.identity.label, host_scope, connection.status
        );
    }
}
