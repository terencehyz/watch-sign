//! watch-sign: sign and install an iOS IPA with an embedded Apple Watch app,
//! entirely from Linux, using the vendored isideload from the iloader Watch fork.
//!
//! Pipeline: Apple ID login (anisette + 2FA) -> register iPhone and paired
//! Watches -> register App IDs (iOS + watchOS) with App Group -> download
//! provisioning profiles (verified to contain the App Group) -> sign ->
//! install companion on iPhone and Watch app on the paired Apple Watch.

use std::path::{Path, PathBuf};

use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection};
use isideload::{
    anisette::remote_v3::{DEFAULT_ANISETTE_V3_URL, RemoteV3AnisetteProvider},
    auth::apple_account::{AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse},
    dev::{
        certificates::DevelopmentCertificate, developer_session::DeveloperSession,
        teams::DeveloperTeam,
    },
    sideload::{SideloaderBuilder, TeamSelection, builder::MaxCertsBehavior},
    util::fs_storage::FsStorage,
};
use rootcause::{option_ext::OptionExt, prelude::*, Report};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
watch-sign — sign + install an IPA with an embedded Watch app (Linux-native)

Usage:
  watch-sign --apple-id EMAIL --ipa PATH [options]

Options:
  --apple-id EMAIL      Apple ID used for signing (required)
  --ipa PATH            IPA containing the iOS app + embedded Watch app (required)
  --bundle-id ID        Rewrite the base bundle identifier before signing
                        (final IDs get .<TeamID> appended by isideload)
  --anisette-url URL    Anisette v3 server (default: https://ani.stikstore.app)
  --state DIR           Private state dir for certs/anisette (default: ~/.local/share/watch-sign)
  --sign-only           Only sign; copy the signed .app to --out (no installation)
  --out DIR             Output dir for --sign-only (default: ./signed-out)
  -h, --help            Show this help

The Apple ID password and 2FA code are prompted interactively and never stored.
";

struct Args {
    apple_id: String,
    ipa: PathBuf,
    bundle_id: Option<String>,
    anisette_url: String,
    state: PathBuf,
    sign_only: bool,
    out: PathBuf,
}

fn parse_args() -> Result<Args, Report> {
    let mut argv = std::env::args().skip(1);
    let mut apple_id = None;
    let mut ipa = None;
    let mut bundle_id = None;
    let mut anisette_url = DEFAULT_ANISETTE_V3_URL.to_string();
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let mut state = PathBuf::from(home).join(".local/share/watch-sign");
    let mut sign_only = false;
    let mut out = PathBuf::from("signed-out");

    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--apple-id" => apple_id = Some(argv.next().context("--apple-id needs a value")?),
            "--ipa" => ipa = Some(PathBuf::from(argv.next().context("--ipa needs a value")?)),
            "--bundle-id" => bundle_id = Some(argv.next().context("--bundle-id needs a value")?),
            "--anisette-url" => {
                anisette_url = argv.next().context("--anisette-url needs a value")?
            }
            "--state" => state = PathBuf::from(argv.next().context("--state needs a value")?),
            "--out" => out = PathBuf::from(argv.next().context("--out needs a value")?),
            "--sign-only" => sign_only = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}\n\n{USAGE}"),
        }
    }

    Ok(Args {
        apple_id: apple_id.context("Missing --apple-id")?,
        ipa: ipa.context("Missing --ipa")?,
        bundle_id,
        anisette_url,
        state,
        sign_only,
        out,
    })
}

fn write_plist(path: &Path, value: &plist::Value) -> Result<(), Report> {
    let file = std::fs::File::create(path).context("create plist file")?;
    plist::to_writer_binary(file, value).context("write binary plist")?;
    Ok(())
}

/// Extract the IPA and optionally rewrite bundle identifiers, returning the
/// path of the companion .app directory (accepted by isideload directly).
fn prepare_app(ipa: &Path, bundle_id: Option<&str>, workdir: &Path) -> Result<PathBuf, Report> {
    let extracted = workdir.join("extracted");
    if extracted.exists() {
        std::fs::remove_dir_all(&extracted).context("clean previous extraction")?;
    }
    std::fs::create_dir_all(&extracted).context("create extraction dir")?;

    let file = std::fs::File::open(ipa).context("open IPA")?;
    let mut archive = zip::ZipArchive::new(file).context("read IPA as zip")?;
    archive.extract(&extracted).context("extract IPA")?;

    let payload = extracted.join("Payload");
    let mut apps: Vec<PathBuf> = std::fs::read_dir(&payload)
        .context("read Payload dir")?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .collect();
    if apps.len() != 1 {
        bail!("Expected exactly one .app in Payload, found {}", apps.len());
    }
    let app = apps.pop().unwrap();

    if let Some(new_base) = bundle_id {
        let info_path = app.join("Info.plist");
        let mut info: plist::Value =
            plist::from_bytes(&std::fs::read(&info_path).context("read Info.plist")?)
                .context("parse Info.plist")?;
        let old_base = info
            .as_dictionary()
            .and_then(|dict| dict.get("CFBundleIdentifier"))
            .and_then(|value| value.as_string())
            .context("Info.plist has no CFBundleIdentifier")?
            .to_string();

        info.as_dictionary_mut()
            .context("Info.plist is not a dictionary")?
            .insert(
                "CFBundleIdentifier".to_string(),
                plist::Value::String(new_base.to_string()),
            );
        write_plist(&info_path, &info)?;

        let watch_dir = app.join("Watch");
        if watch_dir.is_dir() {
            for entry in std::fs::read_dir(&watch_dir).context("read Watch dir")? {
                let watch_app = entry.context("read Watch entry")?.path();
                if !watch_app.extension().is_some_and(|ext| ext == "app") {
                    continue;
                }
                let watch_info_path = watch_app.join("Info.plist");
                let mut watch_info: plist::Value = plist::from_bytes(
                    &std::fs::read(&watch_info_path).context("read Watch Info.plist")?,
                )
                .context("parse Watch Info.plist")?;
                let dict = watch_info
                    .as_dictionary_mut()
                    .context("Watch Info.plist is not a dictionary")?;
                if let Some(old_watch_id) = dict
                    .get("CFBundleIdentifier")
                    .and_then(|value| value.as_string())
                    .map(str::to_string)
                {
                    let new_watch_id = old_watch_id.replacen(&old_base, new_base, 1);
                    dict.insert(
                        "CFBundleIdentifier".to_string(),
                        plist::Value::String(new_watch_id),
                    );
                }
                dict.insert(
                    "WKCompanionAppBundleIdentifier".to_string(),
                    plist::Value::String(new_base.to_string()),
                );
                write_plist(&watch_info_path, &watch_info)?;
            }
        }
        println!("Bundle ID rewritten: {old_base} -> {new_base}");
    }

    Ok(app)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), Report> {
    std::fs::create_dir_all(dst).context("create output dir")?;
    for entry in std::fs::read_dir(src).context("read signed bundle")? {
        let entry = entry.context("read signed bundle entry")?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).context("copy signed bundle file")?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    isideload::init().context("Failed to initialize isideload error reporting")?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = parse_args()?;
    std::fs::create_dir_all(&args.state).context("create state dir")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&args.state, std::fs::Permissions::from_mode(0o700))
            .context("restrict state dir permissions")?;
    }

    let workdir = args.state.join("work");
    std::fs::create_dir_all(&workdir).context("create work dir")?;
    let app_path = prepare_app(&args.ipa, args.bundle_id.as_deref(), &workdir)?;

    let password =
        rpassword::prompt_password("Apple ID password (hidden, never stored): ")
            .context("read password")?;

    let anisette = RemoteV3AnisetteProvider::new(
        &args.anisette_url,
        Box::new(FsStorage::new(args.state.clone())),
        "2".to_string(),
    )
    .context("create anisette provider")?;

    let get_2fa_code = async |params: TwoFactorCallbackParams| {
        let mut code = String::new();
        if params.unknown {
            println!("The previous 2FA method failed. Enter 'd' to send to trusted devices, or a code.");
        } else if params.sms {
            println!("Enter the 2FA code sent by SMS, or 'd' for trusted devices, 'r' to resend:");
        } else {
            println!("Enter the 2FA code shown on your trusted devices (or 'r' to resend):");
        }
        std::io::stdin()
            .read_line(&mut code)
            .context("read 2FA input")?;
        let trimmed = code.trim();
        if trimmed == "d" {
            return Ok(TwoFactorCallbackResponse::SendToDevices);
        }
        if trimmed == "r" && !params.unknown {
            return Ok(TwoFactorCallbackResponse::ResendCode);
        }
        if let Some(rest) = trimmed.strip_prefix('p')
            && let Ok(id) = rest.parse::<u32>()
        {
            return Ok(TwoFactorCallbackResponse::SendSms(id));
        }
        Ok(TwoFactorCallbackResponse::SubmitCode(trimmed.to_string()))
    };

    println!("Logging in Apple ID {} via anisette {} ...", args.apple_id, args.anisette_url);
    let mut account = AppleAccount::builder(&args.apple_id)
        .anisette_provider(anisette)
        .login(&password, get_2fa_code)
        .await
        .context("Apple ID login failed")?;
    drop(password);
    println!("Login OK.");

    let dev_session = DeveloperSession::from_account(&mut account)
        .await
        .context("Failed to create developer session")?;

    let mut usbmuxd = UsbmuxdConnection::default()
        .await
        .context("Failed to connect to usbmuxd (is the iPhone plugged in?)")?;
    let devices = usbmuxd.get_devices().await.context("Failed to list USB devices")?;
    if devices.is_empty() {
        bail!("No iPhone found over USB. Connect the paired iPhone, unlock it, and trust this computer.");
    }
    // macOS usbmuxd also lists Wi-Fi-paired devices; installs must go over the
    // cable, so prefer a USB entry over a network one.
    let device = devices
        .iter()
        .find(|device| device.connection_type == Connection::Usb)
        .or_else(|| devices.first())
        .unwrap();
    if device.connection_type != Connection::Usb {
        println!(
            "Warning: no USB connection for {} (using {:?}); installs may be slow or fail. Plug in the cable.",
            device.udid, device.connection_type
        );
    }
    println!("Using device {} ({:?})", device.udid, device.connection_type);
    let provider = device.to_provider(
        UsbmuxdAddr::from_env_var().context("usbmuxd address")?,
        "watch-sign",
    );

    let cert_selection_prompt = |certs: &Vec<DevelopmentCertificate>| {
        println!("Maximum number of development certificates reached. Select certificates to revoke:");
        for (index, cert) in certs.iter().enumerate() {
            println!(
                "  ({}) {}: {}",
                index + 1,
                cert.name.as_deref().unwrap_or("<Unnamed>"),
                cert.machine_name.as_deref().unwrap_or("<No Machine Name>"),
            );
        }
        println!("Enter numbers separated by commas (empty to cancel):");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok()?;
        let selections: Vec<usize> = input
            .trim()
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&n| n > 0 && n <= certs.len())
            .collect();
        if selections.is_empty() {
            return None;
        }
        Some(
            selections
                .into_iter()
                .map(|n| certs[n - 1].serial_number.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
        )
    };

    let team_selection_prompt = |teams: &Vec<DeveloperTeam>| {
        println!("Multiple developer teams found, select one:");
        for (index, team) in teams.iter().enumerate() {
            println!(
                "  {}: {} ({})",
                index + 1,
                team.name.as_deref().unwrap_or("<Unnamed>"),
                team.team_id
            );
        }
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok()?;
        let selection = input.trim().parse::<usize>().ok()?;
        if selection == 0 || selection > teams.len() {
            return None;
        }
        Some(teams[selection - 1].team_id.clone())
    };

    let mut sideloader = SideloaderBuilder::new(dev_session, args.apple_id.clone())
        .team_selection(TeamSelection::PromptOnce(team_selection_prompt))
        .max_certs_behavior(MaxCertsBehavior::Prompt(Box::new(cert_selection_prompt)))
        .storage(Box::new(FsStorage::new(args.state.clone())))
        .machine_name("uos-watch-sign".to_string())
        .delete_app_after_install(false)
        .build();

    let progress = |p: f32| async move {
        println!("progress: {:.0}%", p * 100.0);
    };

    if args.sign_only {
        let (signed_path, _special) = sideloader
            .sign_app(app_path, None, false, Some(progress))
            .await
            .context("Failed to sign app")?;
        copy_dir(&signed_path, &args.out)?;
        println!("Signed app saved to: {}", args.out.display());
        println!("Install the Watch app with: watch_device.py --install <out>/Watch/<WatchApp>.app");
    } else {
        sideloader
            .install_app(&provider, app_path, false, Some(progress))
            .await
            .context("Failed to sign and install app")?;
        println!("Done: companion app installed on iPhone, Watch app installed on the Apple Watch.");
        println!("Open the app on the Watch and follow its steps.");
    }

    Ok(())
}