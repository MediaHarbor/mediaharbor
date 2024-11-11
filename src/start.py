import subprocess
import shutil
import sys
import os
import platform
from pathlib import Path

def restart_script():
    """Restart the current script with the same arguments"""
    console.print("[yellow]Restarting script to load newly installed packages...[/yellow]")
    os.execv(sys.executable, [sys.executable] + sys.argv)

try:
    from rich.console import Console
    from rich.text import Text
    from rich.theme import Theme
    from rich import print as rich_print
except ImportError:
    print("Rich package not found. Installing...")

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
        print("Rich package installed successfully. Restarting script...")
        restart_script()
    except subprocess.CalledProcessError:
        print("Failed to install 'rich' package. Please install it manually.")
        sys.exit(1)

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

def add_to_path(new_path):
    """Add a new path to system PATH"""
    if platform.system().lower() == "windows":
        path_var = os.environ.get("PATH", "")
        if new_path not in path_var:
            os.environ["PATH"] = f"{new_path};{path_var}"
    else:
        path_var = os.environ.get("PATH", "")
        if new_path not in path_var:
            os.environ["PATH"] = f"{new_path}:{path_var}"


        shell_rc = os.path.expanduser("~/.bashrc")
        if os.path.exists(os.path.expanduser("~/.zshrc")):
            shell_rc = os.path.expanduser("~/.zshrc")

        with open(shell_rc, 'a+') as f:
            f.seek(0)
            if f"PATH={new_path}" not in f.read():
                f.write(f'\nexport PATH="{new_path}:$PATH"\n')

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

            git_url = "https://sourceforge.net/projects/git-osx-installer/files/latest/download"
            git_installer = "/tmp/git-installer.pkg"
            subprocess.run(["curl", "-L", git_url, "-o", git_installer], check=True)
            subprocess.run(["sudo", "installer", "-pkg", git_installer, "-target", "/"], check=True)
            os.remove(git_installer)

        elif system == "windows":
            try:
                if is_tool_installed("winget"):
                    subprocess.run(["winget", "install", "Git.Git", "-e", "--silent"], check=True)
                else:
                    ps_command = r"""
                    $url = "https://github.com/git-for-windows/git/releases/latest/download/Git-2.40.1-64-bit.exe"
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

def install_ffmpeg():
    """Install the actual FFmpeg binary"""
    system = platform.system().lower()
    try:
        if system == "linux":
            if is_tool_installed("apt-get"):
                subprocess.run(["sudo", "apt-get", "update", "-y"], check=True)
                subprocess.run(["sudo", "apt-get", "install", "ffmpeg", "-y"], check=True)
            elif is_tool_installed("dnf"):
                subprocess.run(["sudo", "dnf", "install", "ffmpeg", "-y"], check=True)
            elif is_tool_installed("yum"):
                subprocess.run(["sudo", "yum", "install", "epel-release", "-y"], check=True)
                subprocess.run(["sudo", "yum", "install", "ffmpeg", "-y"], check=True)
            elif is_tool_installed("pacman"):
                            subprocess.run(["sudo", "pacman", "-S", "ffmpeg", "--noconfirm"], check=True)
        elif system == "darwin":

            ffmpeg_url = "https://evermeet.cx/ffmpeg/ffmpeg-7.1.zip"
            ffmpeg_zip = "/tmp/ffmpeg.zip"
            ffmpeg_extract_path = "/usr/local/bin"
            subprocess.run(["curl", "-L", ffmpeg_url, "-o", ffmpeg_zip], check=True)
            subprocess.run(["unzip", ffmpeg_zip, "-d", "/tmp/ffmpeg"], check=True)
            ffmpeg_binary = "/tmp/ffmpeg/ffmpeg"
            if not os.path.exists(ffmpeg_binary):
                ffmpeg_binary = "/tmp/ffmpeg/ffmpeg-macos"
            shutil.move(ffmpeg_binary, os.path.join(ffmpeg_extract_path, "ffmpeg"))
            os.chmod(os.path.join(ffmpeg_extract_path, "ffmpeg"), 0o755)
            os.remove(ffmpeg_zip)
            shutil.rmtree("/tmp/ffmpeg")

        elif system == "windows":
            if is_tool_installed("winget"):
                subprocess.run(["winget", "install", "Gyan.FFmpeg", "-e", "--silent"], check=True)
            else:

                ps_command = """
                $url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
                $output = "$env:TEMP\\ffmpeg.zip"
                $extractPath = "$env:ProgramFiles\\ffmpeg"
                Invoke-WebRequest -Uri $url -OutFile $output
                Expand-Archive -Path $output -DestinationPath $extractPath -Force
                $ffmpegPath = (Get-ChildItem -Path $extractPath -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1).DirectoryName
                [Environment]::SetEnvironmentVariable("Path", $env:Path + ";$ffmpegPath", [System.EnvironmentVariableTarget]::User)
                $env:Path = [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::User)
                Remove-Item $output
                """
                subprocess.run(["powershell", "-Command", ps_command], check=True)


        if system != "windows":
            ffmpeg_path = subprocess.run(["which", "ffmpeg"], capture_output=True, text=True).stdout.strip()
            if ffmpeg_path:
                add_to_path(os.path.dirname(ffmpeg_path))

    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install FFmpeg: {e}[/bold red]")
        return False
    return True

def install_mp4decrypt():
    """Install mp4decrypt binary"""
    system = platform.system().lower()
    try:
        if system == "linux":

            subprocess.run([
                "wget",
                "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-unknown-linux.zip",
                "-O", "/tmp/bento4.zip"
            ], check=True)
            subprocess.run(["unzip", "/tmp/bento4.zip", "-d", "/tmp/bento4"], check=True)
            os.makedirs(os.path.expanduser("~/.local/bin"), exist_ok=True)
            shutil.copy2("/tmp/bento4/bin/mp4decrypt", os.path.expanduser("~/.local/bin/"))
            os.chmod(os.path.expanduser("~/.local/bin/mp4decrypt"), 0o755)
            add_to_path(os.path.expanduser("~/.local/bin"))

        elif system == "darwin":

            bento4_url = "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.universal-apple-macosx.zip"
            bento4_zip = "/tmp/bento4.zip"
            bento4_extract_path = "/usr/local/bin"
            subprocess.run(["curl", "-L", bento4_url, "-o", bento4_zip], check=True)
            subprocess.run(["unzip", bento4_zip, "-d", "/tmp/bento4"], check=True)
            mp4decrypt_binary = "/tmp/bento4/bin/mp4decrypt"
            shutil.move(mp4decrypt_binary, os.path.join(bento4_extract_path, "mp4decrypt"))
            os.chmod(os.path.join(bento4_extract_path, "mp4decrypt"), 0o755)
            os.remove(bento4_zip)
            shutil.rmtree("/tmp/bento4")

        elif system == "windows":
            ps_command = """
            $url = "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-microsoft-win32.zip"
            $output = "$env:TEMP\\bento4.zip"
            $extractPath = "$env:ProgramFiles\\Bento4"
            Invoke-WebRequest -Uri $url -OutFile $output
            Expand-Archive -Path $output -DestinationPath $extractPath -Force
            $bento4Path = (Get-ChildItem -Path $extractPath -Recurse -Filter "mp4decrypt.exe" | Select-Object -First 1).DirectoryName
            [Environment]::SetEnvironmentVariable("Path", $env:Path + ";$bento4Path", [System.EnvironmentVariableTarget]::User)
            $env:Path = [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::User)
            Remove-Item $output
            """
            subprocess.run(["powershell", "-Command", ps_command], check=True)

    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install mp4decrypt: {e}[/bold red]")
        return False
    return True

def ensure_pip_installed():
    """Ensure pip is installed on the system"""
    if not is_tool_installed("pip") and not is_tool_installed("pip3"):
        system = platform.system().lower()
        if system == "linux":
            try:
                if is_tool_installed("apt-get"):
                    subprocess.run(["sudo", "apt-get", "update", "-y"], check=True)
                    subprocess.run(["sudo", "apt-get", "install", "python3-pip", "-y"], check=True)
                elif is_tool_installed("dnf"):
                    subprocess.run(["sudo", "dnf", "install", "python3-pip", "-y"], check=True)
                elif is_tool_installed("yum"):
                    subprocess.run(["sudo", "yum", "install", "python3-pip", "-y"], check=True)
                elif is_tool_installed("pacman"):
                    subprocess.run(["sudo", "pacman", "-S", "python-pip", "--noconfirm"], check=True)
            except subprocess.CalledProcessError as e:
                console.print(f"[bold red]Failed to install pip: {e}[/bold red]")
                sys.exit(1)
        elif system == "darwin":

            get_pip_url = "https://bootstrap.pypa.io/get-pip.py"
            get_pip_path = "/tmp/get-pip.py"
            subprocess.run(["curl", "-L", get_pip_url, "-o", get_pip_path], check=True)
            subprocess.run([sys.executable, get_pip_path], check=True)
            os.remove(get_pip_path)

def install_tools():
    break_system_packages = is_externally_managed()

    if break_system_packages:
        console.print("[yellow]Detected externally managed environment. Using pip with --break-system-packages for installations...[/yellow]")


    ensure_pip_installed()


    if not is_tool_installed("ffmpeg"):
        console.print("[bold yellow]FFmpeg not found. Installing system FFmpeg...[/bold yellow]")
        if not install_ffmpeg():
            return False


    if not is_tool_installed("mp4decrypt"):
        console.print("[bold yellow]mp4decrypt not found. Installing...[/bold yellow]")
        if not install_mp4decrypt():
            return False


    required_python_packages = ["google-api-python-client", "yt-dlp"]
    for package in required_python_packages:
        if not is_tool_installed(package):
            console.print(f"[bold yellow]{package} not found. Installing...[/bold yellow]")
            install_package(package, break_system_packages=break_system_packages)


    if not is_tool_installed("git"):
        console.print("[bold yellow]git not found. Installing...[/bold yellow]")
        install_git_silent()


    if platform.system().lower() == "linux":
        local_bin = os.path.expanduser("~/.local/bin")
        if local_bin not in os.environ.get("PATH", ""):
            add_to_path(local_bin)

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
    optional_tools = sys.argv[1:]
    install_mp4decrypt_flag = "mp4decrypt" in optional_tools
    packages_to_install = [pkg for pkg in optional_tools if pkg != "mp4decrypt"]

    if install_mp4decrypt_flag:
        console.print("[bold yellow]Installing mp4decrypt as specified...[/bold yellow]")
        install_mp4decrypt()

    if packages_to_install:
        console.print(f"[bold yellow]Installing packages: {packages_to_install}[/bold yellow]")
        install_python_packages(packages_to_install)
    else:
        console.print("[bold yellow]No additional packages provided. Skipping optional installations.[/bold yellow]")

    console.print("[bold green]All tools and packages are set up. Exiting script...[/bold green]")
    sys.exit(0)