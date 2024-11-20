# Manual Installation Guide

I apologize for the first start problems, please create a new issue with details about your operating system.

## Dependencies and PIP Packages

| Component                                | Official Website / Repository                            | Example Installation                                                      |
|------------------------------------------|----------------------------------------------------------|---------------------------------------------------------------------------|
| **Python** (3.9 - 3.12)                  | [python.org](https://www.python.org/)                    | Download and install from [python.org](https://www.python.org/)           |
| **FFmpeg**                               | [ffmpeg.org](https://ffmpeg.org/)                        | Follow installation instructions on [ffmpeg.org](https://ffmpeg.org/)     |
| **Git**                                  | [git-scm.com](https://git-scm.com/)                      | Download and install from [git-scm.com](https://git-scm.com/)             |
| **yt-dlp** (YouTube)                     | [yt-dlp GitHub](https://github.com/yt-dlp/yt-dlp)         | `pip install yt-dlp`                                                      |
| **youtubemusicapi**  (YTMusic Search)    | [youtubemusicapi GitHub](https://github.com/sigma67/youtube-music-api) | `pip install youtubemusicapi`                                               |
| **custom_streamrip** (Qobuz)             | [custom_streamrip](https://github.com/mediaharbor/custom_streamrip.git) | `pip install git+https://github.com/mediaharbor/custom_streamrip.git`     |
| **custom_streamrip** (Deezer)            | [custom_streamrip](https://github.com/mediaharbor/custom_streamrip.git) | `pip install git+https://github.com/mediaharbor/custom_streamrip.git`     |
| **custom_streamrip** (Tidal)             | [custom_streamrip](https://github.com/mediaharbor/custom_streamrip.git) | `pip install git+https://github.com/mediaharbor/custom_streamrip.git`     |
| **custom_votify** (Spotify)              | [custom_votify GitHub](https://github.com/custom_votify)  | `pip install git+https://github.com/custom_votify.git`                    |
| **custom_gamdl** (Apple Music)           | [custom_gamdl GitHub](https://github.com/custom_gamdl)  | `pip install git+https://github.com/custom_gamdl.git`                      |
| **pyapplemusicapi** (Apple Music Search) | [pyapplemusicapi GitHub](https://github.com/someuser/pyapplemusicapi) | `pip install pyapplemusicapi`                                               |

## Why custom_?
Because their downloaders don't support new line downloaders, I modified them. If they add new line progress bars to their CLI (like yt-dlp), we can use them.

