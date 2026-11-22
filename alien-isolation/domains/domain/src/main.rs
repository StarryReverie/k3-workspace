mod subcommand;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generates a new domain project
    New {
        /// The name of the domain project
        #[arg(short, long, value_name = "NAME")]
        name: String,
    },
    /// Build a domain project
    Build {
        /// The name of the domain project
        #[arg(short, long, value_name = "NAME")]
        name: String,
        /// The log level, default is INFO
        #[arg(short, long, value_name = "LOG", default_value = "INFO")]
        log: String,
        /// The output directory
        #[arg(short, long, value_name = "OUTPUT", default_value = "./build")]
        output: String,
    },
    /// Build all domain projects
    BuildAll {
        /// The log level, default is INFO
        #[arg(short, long, value_name = "LOG", default_value = "INFO")]
        log: String,
        /// The output directory
        #[arg(short, long, value_name = "OUTPUT", default_value = "./build")]
        output: String,
    },
    /// Clean a domain project
    Clean {
        /// The name of the domain project
        #[arg(short, long, value_name = "NAME", default_value = "")]
        name: String,
    },
    /// Format a domain project
    Fmt {
        /// The name of the domain project
        #[arg(short, long, value_name = "NAME", default_value = "")]
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Some(Commands::New { name }) => {
            println!("Creating new domain project: {}", name);
            subcommand::new::create_domain(name);
        }
        Some(Commands::BuildAll { log, output }) => {
            println!("Building all domain projects, LOG: {log}");
            subcommand::build::build_all(log.to_string(), output);
        }
        Some(Commands::Build { name, log, output }) => {
            println!("Building domain project: {}, LOG: {}", name, log);
            subcommand::build::build_single(name, log, output);
        }
        Some(Commands::Clean { name }) => {
            println!("Cleaning domain project: {}", name);
            subcommand::clean::clean_domain(name.to_string());
        }
        Some(Commands::Fmt { name }) => {
            println!("Formatting domain project: {}", name);
            subcommand::fmt::fmt_domain(name.to_string());
        }
        None => {}
    }
}
