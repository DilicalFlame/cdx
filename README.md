# cdx - Fast Directory Navigation

**`cdx`** is a blazing-fast, strictly-optimized Rust CLI tool designed to make jumping between deeply nested project directories effortless. Instead of typing out long paths like `cd ../../Playground/data-pipeline`, you simply type `cdx data` and instantly jump there.

## Features

- **Lightning Fast (Parallel Search):** Utilizes multi-threaded directory traversal using the ripgrep (`ignore`) core engine. Threads search concurrently and utilize all CPU cores.
- **Lazy-Loading Pagination:** Automatically pauses filesystem traversal the moment a page fills up (saving CPU/IO), resuming only when you navigate to the next page.
- **Beautiful Custom TUI:** A completely custom terminal interface with colorful path highlights, safe visual borders, and intelligent middle-path truncation for extremely long directory paths.
- **Smart Ignores:** Natively respects standard `.gitignore` files and hidden directories out of the box so it never wastes time scanning `node_modules` or `.git`.
- **Highly Configurable:** Global configuration via `~/.config/cdx.toml` to define max page sizes and explicit heavy folders to skip globally.
- **.cdxignore Support:** Drop a `.cdxignore` file anywhere in your filesystem to create advanced, gitignore-style exclusions for particular nested data dumps or archives.

---

## Installation & Setup

Because `cdx` is an interactive CLI that ultimately changes your shell's working directory, it requires two parts: **1. The Binary**, and **2. The Shell Wrapper**.

### 1. Build the Binary
First, compile the Rust project:
```bash
cargo build --release
```
Then, move the resulting binary (`target/release/cdx.exe` or `cdx` on Linux/macOS) to a directory included in your system's `PATH` (e.g., `C:\tools\cdx.exe` or `/usr/local/bin/cdx`).

### 2. Configure Your Shell (Crucial Step!)
Child processes (like a Rust binary) cannot change the parent shell's working directory directly. To actually navigate, `cdx` relies on a hidden `--out <tmp_file>` argument to pass the selected path back to your shell.

#### **For PowerShell (`$PROFILE`)**
Add the following function to your PowerShell profile (open it with `notepad $PROFILE`):

```powershell
function cdx ($term) {
    # Generate a temporary file to store the selected path
    $tmpPath = [System.IO.Path]::GetTempFileName()
    
    try {
        # Run cdx.exe natively (ensure it's in your PATH)
        cdx.exe $term --out $tmpPath
        
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

*(Note for Linux/Mac bash/zsh users: A similar shell function can be written creating a temp file with `mktemp`, executing `cdx $1 --out $tmp`, and capturing the cd result).*

---

## Usage

Simply type `cdx` followed by the prefix of the folder you want to find.

```powershell
cdx data
```

**TUI Controls:**
- `Up` / `Down` : Navigate up and down the current page of results.
- `Left` / `Right` : Snap instantly between pages. Pushing right fetches the next set of results automatically.
- `Enter` : Confirm your selection (your shell will immediately `cd` to the directory).
- `Esc` / `q` / `c` : Abort and exit.

---

## Configuration

Upon first run, `cdx` generates a cross-platform configuration file located at:
**`~/.config/cdx.toml`** *(e.g., `C:\Users\username\.config\cdx.toml`)*

### Default `cdx.toml`
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

### Expanding ignored folders
To make `cdx` as fast as possible across your entire drive, consider adding large cache and build directories to this list (e.g., `"build"`, `"dist"`, `"__pycache__"`, `"AppData"`).

### Local `.cdxignore` Rules
If you have massive folders specific to certain areas of your disk, you can drop a `.cdxignore` file right next to them. 

**Example `.cdxignore`:**
```text
# Ignore a specific folder and all its contents
my_massive_experiment/

# Ignore any subfolder ending in _old
*_old/
```
The inner crawler engine will identify this file and effortlessly prune those branches from the search entirely. 

---

## Architecture details
`cdx` takes advantage of Rust's `mpsc::sync_channel` with a bound capacity. Search threads dynamically sleep and suspend CPU activity the exact moment the UI's display buffer is full, minimizing wasted cycles on folders you may never even scroll to! Wait times between massive directory jumps generally clock in under 2-3 milliseconds.