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
    def is_externally_managed():
        try:
            result = subprocess.run(
                [sys.executable, "-m", "pip", "install", "--dry-run", "dummy-package"],
                capture_output=True,
                text=True
            )
            return "externally-managed-environment" in result.stderr
        except subprocess.CalledProcessError:
            return False

    try:
        if is_externally_managed():
            subprocess.run(
                [sys.executable, "-m", "pip", "install", "--break-system-packages", "rich"],
                check=True
            )
        else:
            subprocess.run([sys.executable, "-m", "pip", "install", "rich"], check=True)
    except subprocess.CalledProcessError:
        print("Failed to install 'rich' package. Please install it manually.")
        sys.exit(1)
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
    return shutil.which(name) is not None

def install_git_silent():
    system = platform.system().lower()

    if is_tool_installed("git"):
        print("Git is already installed")
        return True

    try:
        if system == "linux":
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
                brew_install = '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"'
                subprocess.run(brew_install, shell=True, check=True)
            subprocess.run(["brew", "install", "git"], check=True)

        elif system == "windows":
            try:
                if is_tool_installed("winget"):
                    subprocess.run(["winget", "install", "--id", "Git.Git", "-e", "--silent"], check=True)
                else:
                    ps_command = r"""
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

        if is_tool_installed("git"):
            print("Git was successfully installed")
            return True
        else:
            print("Git installation failed")
            return False

    except subprocess.CalledProcessError as e:
        print(f"An error occurred during installation: {e}")
        return False

def is_externally_managed():
    try:
        result = subprocess.run(
            [sys.executable, "-m", "pip", "install", "--dry-run", "dummy-package"],
            capture_output=True,
            text=True
        )
        return "externally-managed-environment" in result.stderr
    except subprocess.CalledProcessError:
        return False

def install_package(package_name, break_system_packages=False):
    try:
        if break_system_packages:
            subprocess.run(
                [sys.executable, "-m", "pip", "install", "--break-system-packages", package_name],
                check=True
            )
        else:
            subprocess.run([sys.executable, "-m", "pip", "install", package_name], check=True)
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install {package_name}: {e}[/bold red]")
        return False

def install_tools():
    break_system_packages = is_externally_managed()

    if break_system_packages:
        console.print("[yellow]Detected externally managed environment. Using pip with --break-system-packages for installations...[/yellow]")
    if not is_tool_installed("googleapi"):
            console.print("[bold yellow]Google python api not found. Installing...[/bold yellow]")
            install_package("googleapi", break_system_packages=break_system_packages)
    if not is_tool_installed("ffmpeg"):
        console.print("[bold yellow]FFmpeg not found. Installing...[/bold yellow]")
        install_package("FFMpeg", break_system_packages=break_system_packages)

    if not is_tool_installed("yt-dlp"):
        console.print("[bold yellow]yt-dlp not found. Installing...[/bold yellow]")
        install_package("yt-dlp", break_system_packages=break_system_packages)

    if not is_tool_installed("git"):
        console.print("[bold yellow]git not found. Installing...[/bold yellow]")
        install_git_silent()

    console.print("[bold green]Installation completed.[/bold green]")
    return True

def install_python_packages(packages):
    break_system_packages = is_externally_managed()
    processed_packages = set()

    for package in packages:
        if package not in processed_packages:
            processed_packages.add(package)
            console.print(f"[bold yellow]Checking for {package}...[/bold yellow]")

            if package.startswith("https://") and package.endswith(".git"):
                package = f"git+{package}"

            result = subprocess.run(
                [sys.executable, "-m", "pip", "show", package],
                capture_output=True,
                text=True
            )
            if "Version" not in result.stdout:
                install_package(package, break_system_packages=break_system_packages)

if __name__ == "__main__":
    print_logo()

    if not install_tools():
        console.print("[bold red]Failed to install required tools. Please check the errors above.[/bold red]")
        sys.exit(1)

    additional_packages = sys.argv[1:]

    if additional_packages:
        console.print(f"[bold yellow]Installing optional packages: {additional_packages}[/bold yellow]")
        install_python_packages(additional_packages)
    else:
        console.print("[bold yellow]No additional packages provided. Skipping optional installations.[/bold yellow]")

    console.print("[bold green]All tools and packages are set up. Exiting script...[/bold green]")
    sys.exit(0)
