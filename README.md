<div align="center">
  <h1>cdx</h1>
  <p><b>A lightning-fast, highly-optimized Rust CLI tool for jumping between nested directories.</b></p>
  <p>
    <img src="https://img.shields.io/badge/Language-Rust-orange.svg" alt="Rust" />
    <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgray.svg" alt="Platforms" />
  </p>
</div>

---

**cdx** is designed to make navigating deeply nested project directories effortless. Instead of typing out long paths like `cd ../../Playground/data-pipeline`, just type `cdx data` to instantly find and jump there.

## Features

- **Blazing Fast Traversal:** Powered by the `ignore` crate (the engine behind `ripgrep`). It intelligently traverses your file system in a deterministic, depth-first alphabetical order.
- **Flexible Matching:** 
  - **Substring:** By default, it uses case-insensitive substring matching (`cdx data` matches `my_data_pipeline`).
  - **Regex:** Use full Regular Expressions with the `-r` flag (`cdx -r "^data.*lab"`).
- **Lazy-Loading Pagination:** Automatically pauses filesystem traversal the moment a page fills up (saving CPU/disk IO), resuming instantly only when you fetch more results.
- **Beautiful Custom TUI:** A sleek terminal interface featuring path highlighting, visual borders, and intelligent middle-truncation (`C:\some\...\target`) to prevent line-wrap breaking.
- **Smart Ignores:** Natively respects `.gitignore` files, global configurations, and hidden directories out of the box. No more wasting time scanning `node_modules` or `.git`.
- **.cdxignore Support:** Drop a `.cdxignore` file anywhere to create dedicated, gitignore-style exclusions for local massive directories.

---

## Installation & Setup

Because cdx is an interactive CLI that changes your shell's working directory, it requires two parts: **1. The Binary**, and **2. The Shell Wrapper**.

### 1. Build the Binary
First, compile the Rust project:
```bash
cargo build --release
```
Then, move the resulting binary (`target/release/cdx.exe` or `cdx` on Linux/macOS) to a directory included in your system's PATH (e.g., `C:\tools\cdx.exe` or `/usr/local/bin/cdx`).

### 2. Configure Your Shell (Crucial Step!)
Child processes (like a Rust binary) cannot change the parent shell's working directory directly. To navigate, cdx relies on a hidden `--out <tmp_file>` argument to pass the selected path back to your shell.

#### **For PowerShell ($PROFILE)**
Add the following snippet to your PowerShell profile (open it with `notepad $PROFILE`):

```powershell
function cdx {
    # Generate a temporary file to store the selected path
    $tmpPath = [System.IO.Path]::GetTempFileName()

    try {
        # Run cdx.exe natively (ensure it's in your PATH) using $args array 
        # so flags like -r are safely passed through to Rust!
        cdx.exe $args --out $tmpPath
        
        # If the file grew in size, navigate to it!
        if ((Get-Item $tmpPath).length -gt 0) {
            $dest = Get-Content $tmpPath -TotalCount 1
            if ($dest -and (Test-Path $dest)) {
                Set-Location $dest
            }
        }
    }
    finally {
        # Always clean up the temp file
        if (Test-Path $tmpPath) {
            Remove-Item $tmpPath -Force
        }
    }
}
```

> **Note for Linux/Mac users:** A similar shell wrapper can be written using `mktemp` and forwarding `$@` to the binary inside your `.bashrc` or `.zshrc`.

---

## Usage

### Default Search (Substring)
Simply type `cdx` followed by part of the folder name you want to find. It matches anywhere in the folder name.
```powershell
cdx data    # Finds "data", "my_data", "DATA_pipeline", etc.
```

### Advanced Search (Regex)
Use the `-r` (or `--regex`) flag to run an advanced regex pattern.
```powershell
cdx -r "^data.*lab$"    # Finds folders that start with 'data' and end with 'lab'
```

### TUI Controls:
| Key | Action |
| --- | --- |
| Up / Down | Navigate up and down the current page of results. |
| Left / Right | Snap instantly between pages. Pushing right fetches the next set. |
| Enter | Confirm your selection and jump to the directory. |
| Esc / q / c | Abort and exit safely. |

---

## Configuration

Upon the first run, cdx generates a cross-platform configuration file located at: 
**~/.config/cdx.toml** *(e.g., C:\Users\username\.config\cdx.toml)*

### Default cdx.toml
```toml
page_size = 10
ignored_folders = [
    "node_modules",
    "target",
    ".venv",
    ".idea",
    ".vscode",
    ".git",
]
```

### Custom .cdxignore
If you have massive folders specific to certain areas of your disk, drop a `.cdxignore` file right next to them. The internal crawler will prune these branches entirely.
```text
# Ignore a specific folder and all its contents
my_massive_experiment/

# Ignore any subfolder ending in _old
*_old/
```

---

## Architecture Details
cdx takes advantage of Rust's `mpsc::sync_channel` with a bounded capacity. The search thread dynamically sleeps and suspends CPU & file I/O activity the exact moment the TUI's display buffer is full. It evaluates the file tree deterministically in alphabetical order, ensuring clean and predictable folder groupings without suffering heavy upfront traversal performance penalties.
