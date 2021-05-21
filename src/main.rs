use chrono::Local;
use clap::{crate_version, App, Arg};
use env_logger::{Builder, Target};
use log::LevelFilter;
use std::io::Write;
use std::error;
use db::{DB, transfer};
//use bson::doc;
use std::sync::Arc;
use tokio::sync::Semaphore;

mod db;

type BoxResult<T> = std::result::Result<T, Box<dyn error::Error + Send + Sync>>;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> BoxResult<()> {
    let opts = App::new("mongodb-stream-rs")
        .version(crate_version!())
        .author("Daniel F. <dan@findelabs.com>")
        .about("Stream MongoDB to MongoDB")
        .arg(
            Arg::with_name("source_uri")
                .long("source_uri")
                .required(true)
                .value_name("STREAM_SOURCE")
                .env("STREAM_SOURCE")
                .help("Source MongoDB URI")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("destination_uri")
                .long("destination_uri")
                .required(true)
                .value_name("STREAM_DEST")
                .env("STREAM_DEST")
                .help("Destination MongoDB URI")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("db")
                .short("d")
                .long("db")
                .required(true)
                .value_name("MONGODB_DB")
                .env("MONGODB_DB")
                .help("MongoDB Database")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("collection")
                .short("c")
                .long("collection")
                .required(false)
                .value_name("MONGODB_COLLECTION")
                .env("MONGODB_COLLECTION")
                .help("MongoDB Collection")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bulk")
                .short("b")
                .long("bulk")
                .required(false)
                .value_name("STREAM_BULK")
                .env("STREAM_BULK")
                .help("Bulk stream documents")
                .conflicts_with("nobulk")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("restart")
                .short("r")
                .long("restart")
                .required(false)
                .value_name("STREAM_RESTART")
                .env("STREAM_RESTART")
                .help("Restart streaming")
                .takes_value(false)
        )
        .arg(
            Arg::with_name("nobulk")
                .short("n")
                .long("nobulk")
                .required(false)
                .value_name("STREAM_NOBULK")
                .env("STREAM_NOBULK")
                .help("Do not upload docs in batches")
                .conflicts_with("bulk")
                .takes_value(false)
        )
        .get_matches();

    // Initialize log Builder
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{{\"date\": \"{}\", \"level\": \"{}\", \"message\": \"{}\"}}",
                Local::now().format("%Y-%m-%dT%H:%M:%S:%f"),
                record.level(),
                record.args()
            )
        })
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    // Create vars for required variables
    let source = &opts.value_of("source_uri").unwrap();
    let destination= &opts.value_of("destination_uri").unwrap();
    let db = &opts.value_of("db").unwrap();

    println!(
        "Starting mongodb-stream-rs:{}", 
        crate_version!(),
    );

    // Create connections to source and destination db's
    let source_db = DB::init(&source, &db).await?;
    let destination_db = DB::init(&destination, &db).await?;


    let collections = match &opts.is_present("collection") {
        true => {
            let mut vec: Vec<String> = Vec::new();
            let coll = &opts.value_of("collection").unwrap();
            vec.push(coll.to_string());
            vec
        },
        false => source_db.collections().await?
    };

    // Create vector for handles
    let mut handles = vec![];

    // Let's rate limit to just 4 collections at once
    let sem = Arc::new(Semaphore::new(4));

    // Loop over collections and start uploading
    for collection in collections {

        let source = source_db.clone();
        let destination = destination_db.clone();
        let opts = opts.clone();

        // Get permission to kick off task
        let permit = Arc::clone(&sem).acquire_owned().await;

        handles.push(tokio::spawn(async move {
            let _permit = permit;
            match transfer(source, destination, opts, collection).await {
                Ok(_) => log::debug!("Thread shutdown"),
                Err(e) => log::error!("Thread error: {}", e)
            }
        }));

    };

    // Join all handles
    futures::future::join_all(handles).await;

    Ok(())
}
