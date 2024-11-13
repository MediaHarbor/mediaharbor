import ctypes
import subprocess
import shutil
import sys
import os
import platform
from pathlib import Path
import time

def basic_print(message, color=None):
    """Fallback print function when rich is not available"""

    colors = {
        'red': '\033[91m',
        'green': '\033[92m',
        'yellow': '\033[93m',
        'reset': '\033[0m'
    }

    if color and color in colors:
        print(f"{colors[color]}{message}{colors['reset']}")
    else:
        print(message)

def restart_script():
    """Restart the current script with the same arguments"""
    basic_print("Restarting script to load newly installed packages...", "yellow")
    os.execv(sys.executable, [sys.executable] + sys.argv)

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
    from rich.console import Console
    from rich.text import Text
    from rich.theme import Theme
    from rich import print as rich_print
    console = Console()
except ImportError:
    basic_print("Rich package not found. Installing...", "yellow")
    try:
        if is_externally_managed():
            subprocess.run(
                [sys.executable, "-m", "pip", "install", "--break-system-packages", "rich"],
                check=True
            )
        else:
            subprocess.run([sys.executable, "-m", "pip", "install", "rich"], check=True)
        basic_print("Rich package installed successfully. Restarting script...", "green")
        restart_script()
    except subprocess.CalledProcessError:
        basic_print("Failed to install 'rich' package. Continuing without rich...", "red")

        class BasicConsole:
            def print(self, message, *args, **kwargs):

                message = str(message)
                message = message.replace("[bold]", "").replace("[/bold]", "")
                message = message.replace("[red]", "").replace("[/red]", "")
                message = message.replace("[green]", "").replace("[/green]", "")
                message = message.replace("[yellow]", "").replace("[/yellow]", "")
                basic_print(message)
        console = BasicConsole()

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
    if platform.system().lower() == "windows":

        if name == "git":
            common_git_paths = [
                r"C:\Program Files\Git\cmd\git.exe",
                r"C:\Program Files (x86)\Git\cmd\git.exe",
                os.path.expandvars(r"%PROGRAMFILES%\Git\cmd\git.exe"),
                os.path.expandvars(r"%LOCALAPPDATA%\Programs\Git\cmd\git.exe")
            ]
            return any(os.path.exists(path) for path in common_git_paths) or shutil.which(name) is not None
    return shutil.which(name) is not None

def get_bento4_paths(system):
    """Get system-specific Bento4 paths"""
    if system == "linux":

        sudo_user = os.environ.get('SUDO_USER', os.getlogin())
        install_dir = str(Path.home()) if os.geteuid() != 0 else str(Path(f"/home/{sudo_user}/.local/bin"))
        return {
            'download_url': "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-unknown-linux.zip",
            'install_dir': install_dir,
            'binary_name': "mp4decrypt",
            'extract_path': "/tmp/bento4",
            'bin_path': "Bento4-SDK-1-6-0-641.x86_64-unknown-linux/bin"
        }
    elif system == "darwin":
        return {
            'download_url': "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.universal-apple-macosx.zip",
            'install_dir': "/usr/local/bin",
            'binary_name': "mp4decrypt",
            'extract_path': "/tmp/bento4",
            'bin_path': "Bento4-SDK-1-6-0-641.universal-apple-macosx/bin"
        }
    else:
        return {
            'download_url': "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-microsoft-win32.zip",
            'install_dir': os.path.expandvars("%PROGRAMFILES%\\Bento4\\bin"),
            'binary_name': "mp4decrypt.exe",
            'extract_path': os.path.expandvars("%TEMP%\\bento4"),
            'bin_path': "Bento4-SDK-1-6-0-641.x86_64-microsoft-win32/bin"
        }

def get_user_home():
    """Cross-platform way to get user's home directory and username"""
    if platform.system().lower() == "windows":
        return {
            'home': Path.home(),
            'username': os.getenv('USERNAME'),
            'is_admin': ctypes.windll.shell32.IsUserAnAdmin() != 0
        }
    else:
        import pwd
        username = os.environ.get('SUDO_USER', os.getlogin())
        try:
            user_info = pwd.getpwnam(username)
            home_dir = Path(user_info.pw_dir)
        except KeyError:
            home_dir = Path.home()
        return {
            'home': home_dir,
            'username': username,
            'is_admin': os.geteuid() == 0
        }

def add_to_path_windows(new_path):
    """Add a new path to Windows system PATH and make it persistent"""
    import winreg


    new_path = os.path.normpath(new_path)


    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_READ | winreg.KEY_WRITE) as key:
        try:
            path_value, _ = winreg.QueryValueEx(key, "Path")
            current_paths = path_value.split(";")
        except WindowsError:
            current_paths = []


    if new_path not in current_paths:
        new_path_value = ";".join(filter(None, current_paths + [new_path]))


        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_WRITE) as key:
            winreg.SetValueEx(key, "Path", 0, winreg.REG_EXPAND_SZ, new_path_value)


        os.environ["PATH"] = new_path + os.pathsep + os.environ.get("PATH", "")


        try:
            import ctypes
            HWND_BROADCAST = 0xFFFF
            WM_SETTINGCHANGE = 0x1A
            SMTO_ABORTIFHUNG = 0x0002
            result = ctypes.c_long()
            ctypes.windll.user32.SendMessageTimeoutW(
                HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment",
                SMTO_ABORTIFHUNG, 5000, ctypes.byref(result)
            )
        except Exception as e:
            console.print(f"[yellow]Warning: Could not notify Windows of PATH change: {e}[/yellow]")

        console.print(f"[green]Added {new_path} to PATH[/green]")
    else:
        console.print(f"[yellow]{new_path} is already in PATH[/yellow]")

def add_to_path_unix(new_path):
    """Add a new path to Unix-like system PATH and make it persistent"""
    user_info = get_user_home()
    user_home = user_info['home']


    shell_configs = [
        user_home / ".bashrc",
        user_home / ".zshrc",
        user_home / ".profile"
    ]

    user_shell_rc = next((conf for conf in shell_configs if conf.exists()), shell_configs[0])


    if new_path not in os.environ.get("PATH", ""):
        os.environ["PATH"] = f"{new_path}:{os.environ.get('PATH', '')}"


    path_export_line = f'export PATH="{new_path}:$PATH"'
    with open(user_shell_rc, 'a+') as f:
        f.seek(0)
        config_content = f.read()
        if path_export_line not in config_content:
            f.write(f'\n{path_export_line}\n')
            console.print(f"[green]Added {new_path} to PATH in {user_shell_rc}[/green]")
        else:
            console.print(f"[yellow]{new_path} is already in PATH in {user_shell_rc}[/yellow]")

def add_to_path(new_path):
    """Cross-platform way to add a path to system PATH"""
    if platform.system().lower() == "windows":
        add_to_path_windows(new_path)
    else:
        add_to_path_unix(new_path)

def install_git_silent():
    system = platform.system().lower()

    if is_tool_installed("git"):
        console.print("[green]Git is already installed[/green]")
        return True

    try:
        if system == "windows":
            git_installed = False
            if is_tool_installed("winget"):
                try:
                    console.print("[yellow]Installing Git using Winget...[/yellow]")
                    subprocess.run(["winget", "install", "Git.Git", "-e", "--silent"], check=True)
                    git_cmd_path = r"C:\Program Files\Git\cmd"
                    if os.path.exists(git_cmd_path):
                        add_to_path_windows(git_cmd_path)
                        force_path_refresh()
                        git_installed = True
                except subprocess.CalledProcessError:
                    console.print("[yellow]Winget installation failed, trying alternative method...[/yellow]")

            if not git_installed:
                try:
                    console.print("[yellow]Installing Git using PowerShell...[/yellow]")
                    ps_command = r"""
                    $url = "https://github.com/git-for-windows/git/releases/latest/download/Git-2.40.1-64-bit.exe"
                    $output = "$env:TEMP\GitInstaller.exe"
                    Invoke-WebRequest -Uri $url -OutFile $output
                    Start-Process -FilePath $output -Args "/VERYSILENT /NORESTART /COMPONENTS=icons,ext\reg\shellhere,assoc,assoc_sh" -Wait
                    Remove-Item $output
                    """
                    subprocess.run(["powershell", "-Command", ps_command], check=True)
                    git_cmd_path = r"C:\Program Files\Git\cmd"
                    if os.path.exists(git_cmd_path):
                        add_to_path_windows(git_cmd_path)
                except Exception as e:
                    console.print(f"[yellow]PowerShell installation failed: {e}[/yellow]")
                    console.print("[yellow]Continuing with remaining installations...[/yellow]")
                    return False

        elif system == "darwin":
            try:
                git_url = "https://sourceforge.net/projects/git-osx-installer/files/latest/download"
                git_installer = "/tmp/git-installer.pkg"
                subprocess.run(["curl", "-L", git_url, "-o", git_installer], check=True)
                subprocess.run(["sudo", "installer", "-pkg", git_installer, "-target", "/"], check=True)
                os.remove(git_installer)
            except Exception as e:
                console.print(f"[yellow]Git installation failed: {e}[/yellow]")
                console.print("[yellow]Continuing with remaining installations...[/yellow]")
                return False

        elif system == "linux":
            try:
                if is_tool_installed("apt-get"):
                    subprocess.run(["sudo", "apt-get", "update"], check=True)
                    subprocess.run(["sudo", "apt-get", "install", "git", "-y"], check=True)
                elif is_tool_installed("dnf"):
                    subprocess.run(["sudo", "dnf", "install", "git", "-y"], check=True)
                elif is_tool_installed("yum"):
                    subprocess.run(["sudo", "yum", "install", "git", "-y"], check=True)
                elif is_tool_installed("pacman"):
                    subprocess.run(["sudo", "pacman", "-S", "git", "--noconfirm"], check=True)
            except Exception as e:
                console.print(f"[yellow]Git installation failed: {e}[/yellow]")
                console.print("[yellow]Continuing with remaining installations...[/yellow]")
                return False

        return is_tool_installed("git")

    except Exception as e:
        console.print(f"[yellow]Unexpected error during Git installation: {e}[/yellow]")
        console.print("[yellow]Continuing with remaining installations...[/yellow]")
        return False
def force_path_refresh():
    """Force refresh of system PATH on Windows"""
    if platform.system().lower() == "windows":
        try:

            import ctypes
            HWND_BROADCAST = 0xFFFF
            WM_SETTINGCHANGE = 0x1A
            SMTO_ABORTIFHUNG = 0x0002
            result = ctypes.c_long()
            ctypes.windll.user32.SendMessageTimeoutW(
                HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment",
                SMTO_ABORTIFHUNG, 5000, ctypes.byref(result)
            )


            path = os.environ.get("PATH", "")
            os.environ["PATH"] = path

            console.print("[green]System PATH refreshed[/green]")
        except Exception as e:
            console.print(f"[yellow]Warning: Could not refresh PATH: {e}[/yellow]")

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
                [sys.executable, "-m", "pip", "install", "--break-system-packages", "--user", package_name],
                check=True
            )
        else:
            subprocess.run([sys.executable, "-m", "pip", "install", "--user", package_name], check=True)
        return True
    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install {package_name}: {e}[/bold red]")
        return False


def install_mp4decrypt():
    """Install mp4decrypt binary with proper path management"""
    system = platform.system().lower()
    paths = get_bento4_paths(system)

    try:

        os.makedirs(paths['install_dir'], exist_ok=True)
        os.makedirs(paths['extract_path'], exist_ok=True)

        if system == "windows":
            ps_command = f"""
            $url = "{paths['download_url']}"
            $output = "{paths['extract_path']}.zip"
            Invoke-WebRequest -Uri $url -OutFile $output
            Expand-Archive -Path $output -DestinationPath "{paths['extract_path']}" -Force
            """
            subprocess.run(["powershell", "-Command", ps_command], check=True)
        else:

            if system == "linux":
                subprocess.run([
                    "wget", paths['download_url'],
                    "-O", f"{paths['extract_path']}.zip"
                ], check=True)
            else:
                subprocess.run([
                    "curl", "-L", paths['download_url'],
                    "-o", f"{paths['extract_path']}.zip"
                ], check=True)
            subprocess.run([
                "unzip", f"{paths['extract_path']}.zip",
                "-d", paths['extract_path']
            ], check=True)


        src_path = None
        for root, dirs, files in os.walk(paths['extract_path']):
            if paths['binary_name'] in files:
                src_path = os.path.join(root, paths['binary_name'])
                break

        if src_path and os.path.exists(src_path):
            dst_path = os.path.join(paths['install_dir'], paths['binary_name'])
            shutil.copy2(src_path, dst_path)
            if system != "windows":
                os.chmod(dst_path, 0o755)


            add_to_path(paths['install_dir'])


            if os.path.exists(f"{paths['extract_path']}.zip"):
                os.remove(f"{paths['extract_path']}.zip")
            shutil.rmtree(paths['extract_path'])

            console.print(f"[bold green]Successfully installed mp4decrypt to {paths['install_dir']}[/bold green]")
            return True
        else:
            console.print(f"[bold red]Error: Could not find {paths['binary_name']} binary after extraction[/bold red]")
            return False

    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install mp4decrypt: {e}[/bold red]")
        return False
    except Exception as e:
        console.print(f"[bold red]Unexpected error during mp4decrypt installation: {e}[/bold red]")
        return False

def install_ffmpeg():
    """Install FFmpeg with proper path management"""
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


            ffmpeg_path = subprocess.run(["which", "ffmpeg"], capture_output=True, text=True).stdout.strip()
            if ffmpeg_path:
                ffmpeg_dir = os.path.dirname(ffmpeg_path)
                add_to_path(ffmpeg_dir)
                console.print(f"[green]Added FFmpeg to PATH: {ffmpeg_dir}[/green]")

        elif system == "darwin":
            ffmpeg_install_dir = "/usr/local/bin"
            ffmpeg_url = "https://evermeet.cx/ffmpeg/ffmpeg-7.1.zip"
            ffmpeg_zip = "/tmp/ffmpeg.zip"


            os.makedirs(ffmpeg_install_dir, exist_ok=True)


            console.print("[yellow]Downloading FFmpeg...[/yellow]")
            subprocess.run(["curl", "-L", ffmpeg_url, "-o", ffmpeg_zip], check=True)


            os.makedirs("/tmp/ffmpeg", exist_ok=True)
            console.print("[yellow]Extracting FFmpeg...[/yellow]")
            subprocess.run(["unzip", "-o", ffmpeg_zip, "-d", "/tmp/ffmpeg"], check=True)


            ffmpeg_binary = "/tmp/ffmpeg/ffmpeg"
            if not os.path.exists(ffmpeg_binary):
                ffmpeg_binary = next(Path("/tmp/ffmpeg").glob("ffmpeg*"))


            shutil.copy2(ffmpeg_binary, os.path.join(ffmpeg_install_dir, "ffmpeg"))
            os.chmod(os.path.join(ffmpeg_install_dir, "ffmpeg"), 0o755)


            add_to_path(ffmpeg_install_dir)
            console.print(f"[green]Added FFmpeg to PATH: {ffmpeg_install_dir}[/green]")


            os.remove(ffmpeg_zip)
            shutil.rmtree("/tmp/ffmpeg")

        elif system == "windows":

            program_files = os.environ.get("ProgramFiles", "C:\\Program Files")
            ffmpeg_install_dir = os.path.join(program_files, "ffmpeg", "bin")

            if is_tool_installed("winget"):
                subprocess.run(["winget", "install", "Gyan.FFmpeg", "-e", "--silent"], check=True)

                time.sleep(2)


                possible_paths = [
                    ffmpeg_install_dir,
                    os.path.join(program_files, "FFmpeg", "bin"),
                    os.path.join(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"), "ffmpeg", "bin"),
                    os.path.expandvars("%LOCALAPPDATA%\\Microsoft\\WinGet\\Packages\\Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe\\ffmpeg\\bin")
                ]

                for path in possible_paths:
                    if os.path.exists(os.path.join(path, "ffmpeg.exe")):
                        ffmpeg_install_dir = path
                        break
            else:
                os.makedirs(ffmpeg_install_dir, exist_ok=True)
                ps_command = f"""
                $url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
                $output = "$env:TEMP\\ffmpeg.zip"

                Invoke-WebRequest -Uri $url -OutFile $output
                Expand-Archive -Path $output -DestinationPath "{ffmpeg_install_dir}" -Force
                Remove-Item $output


                $nestedDir = Get-ChildItem "{ffmpeg_install_dir}" -Directory | Select-Object -First 1
                if ($nestedDir) {{
                    Move-Item "$($nestedDir.FullName)\\bin\\*" "{ffmpeg_install_dir}" -Force
                    Remove-Item $nestedDir.FullName -Recurse
                }}
                """
                subprocess.run(["powershell", "-Command", ps_command], check=True)


            add_to_path(ffmpeg_install_dir)
            force_path_refresh()
            console.print(f"[green]Added FFmpeg to PATH: {ffmpeg_install_dir}[/green]")
            return True

    except subprocess.CalledProcessError as e:
        console.print(f"[bold red]Failed to install FFmpeg: {e}[/bold red]")
        return False
    except Exception as e:
        console.print(f"[bold red]Unexpected error during FFmpeg installation: {e}[/bold red]")
        return False

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
    print_logo()
    optional_tools = sys.argv[1:]
    packages_to_install = [pkg for pkg in optional_tools if pkg != "mp4decrypt"]


    if not is_tool_installed("ffmpeg"):
        console.print("[bold yellow]FFmpeg not found. Installing FFmpeg...[/bold yellow]")
        if not install_ffmpeg():
            console.print("[bold red]Failed to install FFmpeg.[/bold red]")
            sys.exit(1)


    optional_tools = []
    seen = set()
    for tool in sys.argv[1:]:
        if tool not in seen:
            optional_tools.append(tool)
            seen.add(tool)

    git_needed = any(pkg.startswith("https://") and pkg.endswith(".git") for pkg in optional_tools)
    git_installed = False

    if git_needed and not is_tool_installed("git"):
        console.print("[bold yellow]Git installation required for repository cloning.[/bold yellow]")
        console.print("[bold yellow]Please install Git manually.[/bold yellow]")
        sys.exit(1)


    if "mp4decrypt" in optional_tools:
        console.print("[bold yellow]Installing mp4decrypt...[/bold yellow]")
        if not install_mp4decrypt():
            console.print("[bold red]Failed to install mp4decrypt.[/bold red]")
        optional_tools.remove("mp4decrypt")

    if packages_to_install:
        console.print(f"[bold yellow]Installing packages: {packages_to_install}[/bold yellow]")
        install_python_packages(packages_to_install)
    else:
        console.print("[bold yellow]No additional packages provided. Skipping optional installations.[/bold yellow]")

    console.print("[bold green]Installation completed successfully.[/bold green]")
    sys.exit(0)