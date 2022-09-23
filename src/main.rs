#![allow(unused)]
pub mod ops;
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant};
use std::convert::AsRef;
use std::path::{PathBuf, Path};
use rusoto_core::Region;
use rusoto_s3::{S3, S3Client};
use rusoto_ec2::{Ec2, Ec2Client};
use structopt::StructOpt;
use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};
use rand::seq::SliceRandom;
use futures::future::Future;

use console::{style, Emoji};


use rand::Rng;



/// Amazon machine image uploader & other miscellaneous utilities.
#[derive(StructOpt, Debug)]
#[structopt(name = "ami-uploader")]
enum Cli {
    /// AMI uploader
    Upload(AmiUploader)
}

static DEFAULT_AWS_BUCKET_KEY: &'static str = "new-ami-source-image";

/// A basic example
#[derive(StructOpt, Debug)]
struct AmiUploader {
    /// AWS region
    #[structopt(short, long, default_value="us-west-2")]
    region: String,
    /// S3 bucket name
    #[structopt(short, long)]
    bucket: String,
    /// S3 object key
    ///
    /// FYI: If you use the default key; the resulting key in S3 will be
    /// suffixed with the image format extension.
    #[structopt(short, long, default_value=DEFAULT_AWS_BUCKET_KEY)]
    key: String,
    /// Filepath of the source image
    /// The file extension must denote the format of the image
    /// (and obviously be supported by AWS).
    #[structopt(short, long, parse(from_os_str))]
    image: PathBuf,
    /// Enhanced Network Adapter support
    ///
    /// Images that support ENA networking have to have it enabled.
    #[structopt(long)]
    ena: bool,
    /// Name of the new AMI image
    #[structopt(short, long)]
    name: String,
}

static PACKAGES: &[&str] = &[
    "fs-events",
    "my-awesome-module",
    "emoji-speaker",
    "wrap-ansi",
    "stream-browserify",
    "acorn-dynamic-import",
];

static LOOKING_GLASS: console::Emoji<'_, '_> = console::Emoji("🔍  ", "");
static TRUCK: console::Emoji<'_, '_> = console::Emoji("🚚  ", "");
static CLIP: console::Emoji<'_, '_> = console::Emoji("🔗  ", "");
static PAPER: console::Emoji<'_, '_> = console::Emoji("📃  ", "");
static SPARKLE: console::Emoji<'_, '_> = console::Emoji("✨ ", ":-)");
static HAND: console::Emoji<'_, '_> = console::Emoji("☞ ", ":-)");


async fn log_section<U, T: Future<Output = U>>(
    range: [usize; 2],
    msg: &str,
    f: impl Fn() -> T,
) -> U {
    let range = format!("[{}/{}]", range[0], range[1]);
    let range = console::style(&range).bold().dim();
    // print!("{}", range);
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(120);
    pb.set_style(
        ProgressStyle::default_spinner()
            // For more spinners check out the cli-spinners project:
            // https://github.com/sindresorhus/cli-spinners/blob/master/spinners.json
            .tick_strings(&[
                "▹▹▹▹▹",
                "▸▹▹▹▹",
                "▹▸▹▹▹",
                "▹▹▸▹▹",
                "▹▹▹▸▹",
                "▹▹▹▹▸",
                "▪▪▪▪▪",
            ])
            .template("{spinner:.blue} {msg}"),
    );
    pb.set_message(format!(
        "{} {}",
        range,
        msg
    ));
    let rs = f().await;
    pb.finish();
    return rs
}


async fn block_on_snapshot_job(client: &Ec2Client, import_task_id: &ops::ImportTaskId) -> ops::SnapshotId {
    let started = Instant::now();
    let long_threshold = Duration::from_secs(60 * 6);
    let mid_threshold = Duration::from_secs(60 * 3);
    let mut has_id: Option<ops::SnapshotId> = None;
    while has_id.is_none() {
        let elapsed = started.elapsed();
        if elapsed >= long_threshold {
            thread::sleep(Duration::from_secs(60 * 2));
        } else if elapsed >= mid_threshold {
            thread::sleep(Duration::from_secs(40));
        } else {
            thread::sleep(Duration::from_secs(20));
        }
        has_id = ops::ec2_describe_import_snapshot_tasks(
            client,
            &import_task_id,
        ).await;
    }
    return has_id.unwrap()
}

async fn run_upload_cmd(upload: AmiUploader) {
    let started = Instant::now();
    let total_steps = 4;
    let region = Region::from_str(&upload.region).expect("valid aws region");
    let s3_client = rusoto_s3::S3Client::new(region.clone());
    let ec2_client = rusoto_ec2::Ec2Client::new(region);
    let source_path = upload.image;
    let new_image_name = upload.name;
    let new_image_ena = upload.ena;
    let file_name = source_path
        .file_name()
        .expect("file extention")
        .to_str()
        .unwrap();
    let format = source_path
        .extension()
        .expect("missing file extension")
        .to_str()
        .unwrap();
    let dest_bucket = upload.bucket;
    let mut dest_object_key = upload.key;
    if dest_object_key.as_str() == DEFAULT_AWS_BUCKET_KEY {
        dest_object_key = format!("{}.{}", dest_object_key, format)
    }
    let _ = log_section([1, total_steps], "Copying to S3...", || {
        ops::s3_put_object(
            &s3_client,
            &dest_bucket,
            &dest_object_key,
            &source_path,
        )
    })
    .await;
    let import_task_id = log_section([2, total_steps], "Importing snapshot...", || {
        ops::ec2_import_snapshot(
            &ec2_client,
            &dest_bucket,
            &dest_object_key,
            format,
        )
    })
    .await;
    let snap_task_id = log_section([3, total_steps], "Waiting on snapshot task queue...", || {
        block_on_snapshot_job(&ec2_client, &import_task_id)
    })
    .await;
    let ami_id = log_section([4, total_steps], "Registering new AWS AMI image...", || {
        ops::ec2_register_image(&ec2_client, &snap_task_id, &new_image_name, &new_image_ena)
    })
    .await;
    println!(
        "{} Done in {}",
        SPARKLE,
        HumanDuration(started.elapsed()),
    );
    let hand = console::style("☞").bold();
    let created_title = console::style("Created AMI ID:").bold().dim();
    let ami_id_display = console::style(&ami_id).bold().bright().underlined();
    println!(
        "{} {} {}",
        hand,
        created_title,
        ami_id_display,
    );
}

#[tokio::main]
pub async fn main() {
    let args = Cli::from_args();
    match args {
        Cli::Upload(upload) => {
            run_upload_cmd(upload).await;
        }
    }
}
