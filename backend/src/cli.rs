use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "ghostpost-backend", about = "Ghostpost backend kernel")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Apply embedded SQLx migrations with the DDL role
    Migrate {
        /// Validate / print pending migrations without applying
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Run API and/or worker runtime
    Serve {
        #[arg(long, value_enum, default_value_t = ServeRole::All)]
        role: ServeRole,
        /// Override BIND_ADDR
        #[arg(long)]
        bind: Option<String>,
    },
    /// Auth administration commands
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Re-encrypt active workos_sessions under the current seal key version
    ResealWorkosSessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ServeRole {
    Api,
    Worker,
    All,
}
