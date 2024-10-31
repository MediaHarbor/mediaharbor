import subprocess
import shutil
import sys
import os
import platform
try:
    from rich.console import Console
    from rich.text import Text
    from rich.theme import Theme
    from rich import print as rich_print
except ImportError:
    subprocess.run([sys.executable, "-m", "pip", "install", "rich"])
    from rich.console import Console
    from rich.text import Text
    from rich.theme import Theme
    from rich import print as rich_print
console = Console()
def print_logo():
    logo = """
                      {{{{{{{{{{{{{{{{
                  {{{{{{{{{{{{{{{{{{{{{{{
                {{{{{{{{{{{{{{{{{{{{{{{{{{{{
              {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
             {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
           {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
          -{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
          {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{{        {{{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{  ||||||  {{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{  ||  ||| {{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{  ||||||  {{{{{{{{{{{{{{{{
         {{{{{{{{{{{{{{{{{        {{{{{{{{{{{{{{{{{
          {{{{{{{{{{{{{{{{{{{{{{{{-{{  {{{{{-{{{{{
           {-{{{{{{{{{{{{{{{{{{{{-{ {{  {{{{{{{{{{
          {{{{{{{{{{{{{{{{{{{{{{{ {{{{{{{-{{{{{{{
                                 {{{{{-    {{{{{
           {{{{{{  {{{{{{{{{{{{{{{  {{{     {{{
           {{{{{{ {{{{{{{{{{{{{{{{   {{{
               {{{{{{{{{{{{{{{{{{{{  {{{       {{{
             {{{{{{{{{{{{{{{{{{{{{{{  {{{      {{{{
              {{{{{{{{{{{{{{{{{{{{{   {{{{     {{{{
                              {{       {{{     {{{
                              {{{{      {{{  {{{{
                               {{{{{    {{{{{{{{
                                  {{{{{{{{{{{{
                                      {{{{{{
    """
    rainbow_colors = [
        "[red]", "[orange3]", "[yellow]", "[green]",
        "[cyan]", "[blue]", "[magenta]"
    ]
    lines = logo.splitlines()
    for idx, line in enumerate(lines):
        color = rainbow_colors[idx % len(rainbow_colors)]
        rich_print(f"{color}{line}[/]")
def is_tool_installed(name):
    """Check if a program is installed and accessible from PATH"""
    return shutil.which(name) is not None

def install_git_silent():
    """Install Git silently based on the operating system"""
    system = platform.system().lower()
    
    if is_tool_installed("git"):
        print("Git is already installed")
        return True
    
    try:
        if system == "linux":
            # Detect package manager
            if is_tool_installed("apt-get"):
                subprocess.run(["sudo", "apt-get", "update", "-y"], check=True)
                subprocess.run(["sudo", "apt-get", "install", "git", "-y"], check=True)
            elif is_tool_installed("dnf"):
                subprocess.run(["sudo", "dnf", "install", "git", "-y"], check=True)
            elif is_tool_installed("yum"):
                subprocess.run(["sudo", "yum", "install", "git", "-y"], check=True)
            elif is_tool_installed("pacman"):
                subprocess.run(["sudo", "pacman", "-S", "git", "--noconfirm"], check=True)
            else:
                print("Unsupported Linux distribution")
                return False
                
        elif system == "darwin":
            if not is_tool_installed("brew"):
                # Install Homebrew first
                brew_install = '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
                subprocess.run(brew_install, shell=True, check=True)
            subprocess.run(["brew", "install", "git"], check=True)
            
        elif system == "windows":
            try:
                # Use winget if available (Windows 10 and later)
                if is_tool_installed("winget"):
                    subprocess.run(["winget", "install", "--id", "Git.Git", "-e", "--silent"], check=True)
                else:
                    # Alternative: Download and install using PowerShell
                    ps_command = """
                    $url = "https://github.com/git-for-windows/git/releases/latest/download/Git-64-bit.exe"
                    $output = "$env:TEMP\GitInstaller.exe"
                    Invoke-WebRequest -Uri $url -OutFile $output
                    Start-Process -FilePath $output -Args "/VERYSILENT /NORESTART" -Wait
                    Remove-Item $output
                    """
                    subprocess.run(["powershell", "-Command", ps_command], check=True)
            except subprocess.CalledProcessError:
                print("Failed to install Git. Please install manually from https://git-scm.com/")
                return False
        
        # Verify installation
        if is_tool_installed("git"):
            print("Git was successfully installed")
            return True
        else:
            print("Git installation failed")
            return False
            
    except subprocess.CalledProcessError as e:
        print(f"An error occurred during installation: {e}")
        return False

def install_tools():
    if not is_tool_installed("yt-dlp"):
        console.print("[bold yellow]yt-dlp not found. Installing...[/bold yellow]")
        subprocess.run([sys.executable, "-m", "pip", "install", "yt-dlp"])

    if not is_tool_installed("git"):
        console.print("[bold yellow]git not found. Installing...[/bold yellow]")
        if sys.platform.startswith("linux"):
            subprocess.run(["sudo", "apt-get", "install", "git", "-y"])
        elif sys.platform == "darwin":
            subprocess.run(["brew", "install", "git"])
        elif os.name == "nt":
            console.print("[bold red]Please install git manually from https://git-scm.com/[/bold red]")

    console.print("[bold green]git and yt-dlp are installed.[/bold green]")
def install_python_packages(packages):
    processed_packages = set()

    for package in packages:
        if package.startswith("https://"):
            package = f"git+{package}"

        if package not in processed_packages:
            processed_packages.add(package)
            console.print(f"[bold yellow]Checking for {package}...[/bold yellow]")
            result = subprocess.run([sys.executable, "-m", "pip", "show", package], capture_output=True, text=True)
            if "Version" not in result.stdout:
                console.print(f"[bold yellow]{package} not found. Installing...[/bold yellow]")
                subprocess.run([sys.executable, "-m", "pip", "install", package])
if __name__ == "__main__":
    print_logo()

    install_tools()

    additional_packages = sys.argv[1:]

    if additional_packages:
        console.print(f"[bold yellow]Installing optional packages: {additional_packages}[/bold yellow]")
        install_python_packages(additional_packages)
    else:
        console.print("[bold yellow]No additional packages provided. Skipping optional installations.[/bold yellow]")

    console.print("[bold green]All tools and packages are set up. Exiting script...[/bold green]")
    sys.exit()
