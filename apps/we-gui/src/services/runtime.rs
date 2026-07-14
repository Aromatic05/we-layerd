use std::{path::Path, process::{Child, Command}, time::Duration};

pub fn command_exists_in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

pub fn try_switch(config_path: &Path) -> bool {
    Command::new("we-layerd").arg("switch").arg("--config").arg(config_path).status().map(|status| status.success()).unwrap_or(false)
}

pub fn send_control(action: &str) -> bool {
    Command::new("we-layerd").arg("ctl").arg(action).status().map(|status| status.success()).unwrap_or(false)
}

pub fn start(config_path: &Path) -> std::io::Result<Child> {
    Command::new("we-layerd")
        .arg("run")
        .arg("--config")
        .arg(config_path)
        .spawn()
}

pub async fn fetch_status() -> Result<String, String> {
    let output = Command::new("we-layerd")
        .arg("ctl")
        .arg("status")
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Ok("status unavailable: daemon returned empty response".to_string())
        } else {
            Ok(text)
        }
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub async fn fetch_outputs() -> Result<Vec<String>, String> {
    let output = Command::new("we-layerd").arg("outputs").output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

pub fn reap(child: &mut Option<Child>) -> Result<(), String> {
    let Some(process) = child.as_mut() else {
        return Ok(());
    };

    match process.try_wait() {
        Ok(Some(_)) => {
            *child = None;
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn stop(child: &mut Option<Child>) -> bool {
    let mut stopped = send_control("stop");
    if let Some(mut child) = child.take() {
        for _ in 0..3 {
            if child.try_wait().map_or(true, |status| status.is_some()) { return true; }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        stopped = true;
    }
    stopped
}
