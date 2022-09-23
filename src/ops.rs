use std::convert::AsRef;
use std::path::{PathBuf, Path};
use rusoto_core::Region;
use rusoto_s3::{S3, S3Client};
use rusoto_ec2::{Ec2, Ec2Client};

pub async fn s3_put_object<P: AsRef<Path>>(
    client: &S3Client,
    bucket_name: &str,
    object_key: &str,
    source_file_path: P,
) {
    let contents = std::fs::read(source_file_path.as_ref())
        .expect("load given source file");
    let req = rusoto_s3::PutObjectRequest {
        bucket: bucket_name.to_owned(),
        key: object_key.to_owned(),
        body: Some(contents.into()),
        ..Default::default()
    };
    let _ = client.put_object(req)
        .await
        .expect("Couldn't PUT object");
}

pub type ImportTaskId = String;
pub type SnapshotId = String;
pub type ImageId = String;

pub async fn ec2_import_snapshot(
    client: &Ec2Client,
    bucket_name: &str,
    object_key: &str,
    format: &str,
) -> ImportTaskId {
    let user_bucket = rusoto_ec2::UserBucket{
        s3_bucket: Some(bucket_name.to_owned()),
        s3_key: Some(object_key.to_owned()),
    };
    let disk_container = rusoto_ec2::SnapshotDiskContainer{
        description: Some(String::from("ami-uploader created this object")),
        format: Some(format.to_owned()),
        user_bucket: Some(user_bucket),
        ..Default::default()
    };
    let req = rusoto_ec2::ImportSnapshotRequest {
        disk_container: Some(disk_container),
        description: Some(String::from("ami-uploader created this object")),
        ..Default::default()
    };
    let res = client.import_snapshot(req)
        .await
        .expect("Couldn't import-snapshot");
    let snap_task_id = res.import_task_id.unwrap();
    return snap_task_id
}

/// Just returns the resulting snapshot ID (if the job is completed).
pub async fn ec2_describe_import_snapshot_tasks(
    client: &Ec2Client,
    id: &ImportTaskId,
) -> Option<SnapshotId> {
    let req = rusoto_ec2::DescribeImportSnapshotTasksRequest {
        import_task_ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let res = client.describe_import_snapshot_tasks(req)
        .await
        .expect("Couldn't import-snapshot")
        .import_snapshot_tasks
        .unwrap()
        .first()
        .unwrap()
        .to_owned();
    let detail = res.snapshot_task_detail.unwrap();
    let status = detail.status.unwrap();
    if status.as_str() == "completed" {
        return Some(detail.snapshot_id.unwrap())
    }
    None
}

pub async fn ec2_register_image(
    client: &Ec2Client,
    snapshot_id: &SnapshotId,
    image_name: &str,
    image_ena: &bool,
) -> ImageId {
    let block_device_mappings = rusoto_ec2::BlockDeviceMapping{
        device_name: Some(String::from("/dev/sda1")),
        ebs: Some(rusoto_ec2::EbsBlockDevice {
            delete_on_termination: Some(true),
            snapshot_id: Some(snapshot_id.to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let request = rusoto_ec2::RegisterImageRequest{
        architecture: Some(String::from("x86_64")),
        root_device_name: Some(String::from("/dev/sda1")),
        virtualization_type: Some(String::from("hvm")),
        block_device_mappings: Some(vec![
            block_device_mappings
        ]),
        name: image_name.to_owned(),
        ena_support: Some(image_ena.to_owned()),
        ..Default::default()
    };
    return client.register_image(request)
        .await
        .expect("Couldn't import-snapshot")
        .image_id
        .unwrap()
}

pub async fn ec2_deregister_image(
    client: &Ec2Client,
    image_id: &ImageId,
) {
    let request = rusoto_ec2::DeregisterImageRequest{
        image_id: image_id.to_owned(),
        ..Default::default()
    };
    client.deregister_image(request)
        .await
        .expect("Couldn't import-snapshot")
}

/// Lookups AMIs created by the given owner and returns the first matching id given AMI name.
pub async fn get_ami_id_from_name(
    client: &Ec2Client,
    ami_name: &str,
) {
    let request = rusoto_ec2::DescribeImagesRequest {
        owners: Some(vec![String::from("self")]),
        ..Default::default()
    };
    let result = client.describe_images(request).await.unwrap();
    let result = result.images
        .unwrap_or(Vec::new())
        .into_iter()
        .find_map(|image: rusoto_ec2::Image| -> Option<ImageId> {
            if let Some(name) = image.name.as_ref() {
                if name == ami_name {
                    return Some(image.image_id.unwrap());
                }
            }
            None
        });
}
