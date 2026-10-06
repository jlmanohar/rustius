// Standalone RDP Worker process powered by IronRDP.
// Handles X.224, TLS, NLA (CredSSP/NTLM), bitmap decoding, and input dispatching
// in an isolated binary with no dependency conflicts with russh.

use anyhow::{Context, Result};
use core::time::Duration;
use ironrdp::connector::{self, Credentials};
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::input::fast_path::FastPathInput;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::rdp::client_info::PerformanceFlags;
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStageBuilder, ActiveStageOutput};
use ironrdp::core::encode_vec;
use ironrdp::graphics::image_processing::PixelFormat;
use sspi::network_client::reqwest_network_client::ReqwestNetworkClient;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_rustls::rustls;
use tracing::{info, warn};

const MAGIC_FRAME: [u8; 4] = [0x52, 0x44, 0x50, 0x46]; // "RDPF"
const MAGIC_CTRL: [u8; 4] = [0x52, 0x44, 0x50, 0x43];  // "RDPC"
const MAGIC_INPUT: [u8; 4] = [0x52, 0x44, 0x50, 0x49]; // "RDPI"

#[derive(Debug, Clone)]
struct WorkerConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    domain: Option<String>,
    width: u32,
    height: u32,
}

fn parse_cli_args() -> Result<WorkerConfig> {
    let args: Vec<String> = std::env::args().collect();
    let mut host = String::from("127.0.0.1");
    let mut port = 3389u16;
    let mut username = String::new();
    let mut password = String::new();
    let mut domain = None;
    let mut width = 1280u32;
    let mut height = 800u32;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" if i + 1 < args.len() => {
                host = args[i + 1].clone();
                i += 2;
            }
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(3389);
                i += 2;
            }
            "--username" if i + 1 < args.len() => {
                username = args[i + 1].clone();
                i += 2;
            }
            "--password" if i + 1 < args.len() => {
                password = args[i + 1].clone();
                i += 2;
            }
            "--domain" if i + 1 < args.len() => {
                domain = Some(args[i + 1].clone());
                i += 2;
            }
            "--width" if i + 1 < args.len() => {
                width = args[i + 1].parse().unwrap_or(1280);
                i += 2;
            }
            "--height" if i + 1 < args.len() => {
                height = args[i + 1].parse().unwrap_or(800);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    Ok(WorkerConfig {
        host,
        port,
        username,
        password,
        domain,
        width,
        height,
    })
}

fn send_ctrl_msg(stdout: &mut std::io::Stdout, json_str: &str) -> Result<()> {
    let bytes = json_str.as_bytes();
    stdout.write_all(&MAGIC_CTRL)?;
    stdout.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stdout.write_all(bytes)?;
    stdout.flush()?;
    Ok(())
}

fn send_frame(
    stdout: &mut std::io::Stdout,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    data: &[u8],
) -> Result<()> {
    stdout.write_all(&MAGIC_FRAME)?;
    stdout.write_all(&x.to_le_bytes())?;
    stdout.write_all(&y.to_le_bytes())?;
    stdout.write_all(&width.to_le_bytes())?;
    stdout.write_all(&height.to_le_bytes())?;
    stdout.write_all(&(data.len() as u32).to_le_bytes())?;
    stdout.write_all(data)?;
    stdout.flush()?;
    Ok(())
}

fn run() -> Result<()> {
    let config = parse_cli_args()?;
    info!("Starting RDP worker for {}:{} (user: {})", config.host, config.port, config.username);

    let mut stdout = std::io::stdout();

    let connector_config = connector::Config {
        desktop_size: connector::DesktopSize {
            width: config.width as u16,
            height: config.height as u16,
        },
        desktop_scale_factor: 0,
        enable_tls: true,
        enable_credssp: true,
        credentials: Credentials::UsernamePassword {
            username: config.username.clone(),
            password: config.password.clone(),
        },
        domain: config.domain.clone(),
        client_build: 0,
        client_name: "Rustius".to_owned(),
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        keyboard_layout: 0,
        ime_file_name: String::new(),
        bitmap: None,
        dig_product_id: String::new(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        #[cfg(windows)]
        platform: MajorPlatformType::WINDOWS,
        #[cfg(not(windows))]
        platform: MajorPlatformType::UNIX,
        hardware_id: None,
        request_data: None,
        autologon: true,
        enable_audio_playback: false,
        performance_flags: PerformanceFlags::empty(),
        license_cache: None,
        timezone_info: ironrdp::pdu::rdp::client_info::TimezoneInfo::default(),
        compression_type: None,
        enable_server_pointer: false,
        pointer_software_rendering: true,
        multitransport_flags: None,
    };

    // Establish TCP connection
    let server_addr = (config.host.as_str(), config.port)
        .to_socket_addrs()?
        .next()
        .context("Failed to resolve server hostname")?;

    let tcp_stream = TcpStream::connect_timeout(&server_addr, Duration::from_secs(10))
        .context(format!("Failed to connect to TCP {}", server_addr))?;
    let _ = tcp_stream.set_nodelay(true);
    // Use generous timeout during connection and authentication handshake
    tcp_stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .context("set_read_timeout failed")?;

    let client_addr = tcp_stream.local_addr()?;
    let mut framed = ironrdp_blocking::Framed::new(tcp_stream);
    let mut connector = connector::ClientConnector::new(connector_config, client_addr);

    info!("Starting X.224 negotiation with server...");
    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector)
        .context("X.224 connect_begin failed")?;

    info!("Upgrading to TLS...");
    let initial_stream = framed.into_inner_no_leftover();
    let (upgraded_stream, server_public_key) =
        tls_upgrade(initial_stream, config.host.clone()).context("TLS upgrade failed")?;

    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);
    let mut upgraded_framed = ironrdp_blocking::Framed::new(upgraded_stream);

    info!("Performing CredSSP (NLA) authentication & finalizing MCS exchange...");
    let mut network_client = ReqwestNetworkClient;
    let connection_result = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut network_client,
        config.host.clone().into(),
        server_public_key,
        None,
    )
    .context("RDP connect_finalize failed")?;

    let actual_w = connection_result.desktop_size.width as u32;
    let actual_h = connection_result.desktop_size.height as u32;
    info!("RDP session established successfully! Desktop resolution: {}x{}", actual_w, actual_h);

    // Switch underlying TCP stream to low latency read timeout for active session loop
    let _ = upgraded_framed.get_inner_mut().0.sock.set_read_timeout(Some(Duration::from_millis(50)));

    send_ctrl_msg(
        &mut stdout,
        &format!(r#"{{"type":"connected","width":{},"height":{}}}"#, actual_w, actual_h),
    )?;

    let mut image = DecodedImage::new(PixelFormat::RgbA32, actual_w as u16, actual_h as u16);

    let mut active_stage = ActiveStageBuilder {
        static_channels: connection_result.static_channels,
        user_channel_id: connection_result.user_channel_id,
        io_channel_id: connection_result.io_channel_id,
        message_channel_id: connection_result.message_channel_id,
        share_id: connection_result.share_id,
        compression_type: connection_result.compression_type,
        enable_server_pointer: connection_result.enable_server_pointer,
        pointer_software_rendering: connection_result.pointer_software_rendering,
    }
    .build();

    // Input handling thread: reads RDPI commands from stdin and stores in thread-safe channel
    let (input_tx, input_rx) = std::sync::mpsc::channel::<InputMsg>();
    let running = Arc::new(AtomicBool::new(true));
    let running_stdin = running.clone();

    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut magic = [0u8; 4];
        while running_stdin.load(Ordering::SeqCst) {
            if stdin.read_exact(&mut magic).is_err() {
                break;
            }
            if magic == MAGIC_INPUT {
                let mut type_buf = [0u8; 1];
                if stdin.read_exact(&mut type_buf).is_err() {
                    break;
                }
                match type_buf[0] {
                    1 => {
                        // Key: [u16 scancode, u8 pressed]
                        let mut key_buf = [0u8; 3];
                        if stdin.read_exact(&mut key_buf).is_ok() {
                            let scancode = u16::from_le_bytes([key_buf[0], key_buf[1]]);
                            let pressed = key_buf[2] != 0;
                            let _ = input_tx.send(InputMsg::Key { scancode, pressed });
                        }
                    }
                    2 => {
                        // Mouse: [u16 x, u16 y, u8 buttons, i8 wheel]
                        let mut mouse_buf = [0u8; 6];
                        if stdin.read_exact(&mut mouse_buf).is_ok() {
                            let x = u16::from_le_bytes([mouse_buf[0], mouse_buf[1]]);
                            let y = u16::from_le_bytes([mouse_buf[2], mouse_buf[3]]);
                            let buttons = mouse_buf[4];
                            let wheel = mouse_buf[5] as i8;
                            let _ = input_tx.send(InputMsg::Mouse { x, y, buttons, wheel });
                        }
                    }
                    3 => {
                        // Disconnect request
                        break;
                    }
                    _ => {}
                }
            }
        }
        running_stdin.store(false, Ordering::SeqCst);
    });

    let mut mut_database = ironrdp_input::Database::new();

    // Main RDP active session loop
    let mut initial_sent = false;
    while running.load(Ordering::SeqCst) {
        // 1. Process pending input events from frontend
        while let Ok(msg) = input_rx.try_recv() {
            let fastpath_events = match msg {
                InputMsg::Key { scancode, pressed } => {
                    let sc = ironrdp_input::Scancode::from_u16(scancode);
                    if pressed {
                        mut_database.apply(std::iter::once(ironrdp_input::Operation::KeyPressed(sc)))
                    } else {
                        mut_database.apply(std::iter::once(ironrdp_input::Operation::KeyReleased(sc)))
                    }
                }
                InputMsg::Mouse { x, y, buttons, wheel: _ } => {
                    let mut ops = Vec::new();
                    ops.push(ironrdp_input::Operation::MouseMove(ironrdp_input::MousePosition { x, y }));
                    if buttons & 0x01 != 0 {
                        ops.push(ironrdp_input::Operation::MouseButtonPressed(ironrdp_input::MouseButton::Left));
                    } else {
                        ops.push(ironrdp_input::Operation::MouseButtonReleased(ironrdp_input::MouseButton::Left));
                    }
                    if buttons & 0x02 != 0 {
                        ops.push(ironrdp_input::Operation::MouseButtonPressed(ironrdp_input::MouseButton::Right));
                    } else {
                        ops.push(ironrdp_input::Operation::MouseButtonReleased(ironrdp_input::MouseButton::Right));
                    }
                    mut_database.apply(ops)
                }
            };

            for event in fastpath_events {
                let pdu = FastPathInput::single(event);
                if let Ok(buf) = encode_vec(&pdu) {
                    let _ = upgraded_framed.write_all(&buf);
                }
            }
        }

        // 2. Read incoming PDU from server
        let (action, payload) = match upgraded_framed.read_pdu() {
            Ok(res) => res,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Short read timeout so we can process input events promptly
                continue;
            }
            Err(e) => {
                info!("RDP session stream closed: {:?}", e);
                break;
            }
        };

        // 3. Process frame with IronRDP ActiveStage
        match active_stage.process(&mut image, action, &payload) {
            Ok(outputs) => {
                let mut has_update = false;
                for out in outputs {
                    match out {
                        ActiveStageOutput::ResponseFrame(frame) => {
                            if let Err(e) = upgraded_framed.write_all(&frame) {
                                warn!("Failed to write RDP response frame: {:?}", e);
                                break;
                            }
                        }
                        ActiveStageOutput::GraphicsUpdate(_rect) => {
                            has_update = true;
                        }
                        ActiveStageOutput::Terminate(reason) => {
                            info!("RDP server requested terminate: {:?}", reason);
                            running.store(false, Ordering::SeqCst);
                            break;
                        }
                        _ => {}
                    }
                }

                if has_update || !initial_sent {
                    // Send full image buffer or dirty region to frontend
                    initial_sent = true;
                    if let Err(e) = send_frame(&mut stdout, 0, 0, actual_w, actual_h, image.data()) {
                        warn!("Failed to stream frame to parent process: {:?}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                warn!("ActiveStage processing error: {:?}", e);
            }
        }
    }

    send_ctrl_msg(&mut stdout, r#"{"type":"disconnected","code":0}"#)?;
    info!("RDP worker exited cleanly");
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rustius_rdp_worker=debug".into()),
        )
        .init();

    if let Err(e) = run() {
        let err_msg = format!("{:#}", e);
        eprintln!("RDP worker encountered error: {}", err_msg);
        let escaped_err = serde_json::to_string(&err_msg).unwrap_or_else(|_| format!("\"{}\"", err_msg));
        let _ = send_ctrl_msg(
            &mut std::io::stdout(),
            &format!(r#"{{"type":"error","message":{}}}"#, escaped_err),
        );
        std::process::exit(1);
    }
}

#[allow(dead_code)]
enum InputMsg {
    Key { scancode: u16, pressed: bool },
    Mouse { x: u16, y: u16, buttons: u8, wheel: i8 },
}

fn tls_upgrade(
    stream: TcpStream,
    server_name: String,
) -> Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Vec<u8>)> {
    let mut config = rustls::client::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification))
        .with_no_client_auth();

    config.resumption = rustls::client::Resumption::disabled();
    let config = Arc::new(config);

    let server_name_type = server_name.try_into()?;
    let client = rustls::ClientConnection::new(config, server_name_type)?;
    let mut tls_stream = rustls::StreamOwned::new(client, stream);

    tls_stream.flush()?;

    let cert = tls_stream
        .conn
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .context("Server peer certificate is missing")?;

    let server_public_key = extract_tls_server_public_key(cert)?;
    Ok((tls_stream, server_public_key))
}

fn extract_tls_server_public_key(cert: &[u8]) -> Result<Vec<u8>> {
    use x509_cert::der::Decode as _;
    let cert = x509_cert::Certificate::from_der(cert)?;
    let server_public_key = cert
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .context("subject public key BIT STRING is not aligned")?
        .to_owned();
    Ok(server_public_key)
}

mod danger {
    use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use tokio_rustls::rustls::{pki_types, DigitallySignedStruct, Error, SignatureScheme};

    #[derive(Debug)]
    pub(super) struct NoCertificateVerification;

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &pki_types::CertificateDer<'_>,
            _intermediates: &[pki_types::CertificateDer<'_>],
            _server_name: &pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: pki_types::UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &pki_types::CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &pki_types::CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA1,
                SignatureScheme::ECDSA_SHA1_Legacy,
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP521_SHA512,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
                SignatureScheme::ED448,
            ]
        }
    }
}
