![Logo](assets/MediaHarborBanner.svg)

# MediaHarbor Desktop

![License](https://img.shields.io/github/license/MediaHarbor/MediaHarbor) ![Build Status](https://img.shields.io/github/actions/workflow/status/MediaHarbor/MediaHarbor/node.js.yml) ![Downloads](https://img.shields.io/github/downloads/MediaHarbor/MediaHarbor/latest/total)
![Static Badge](https://img.shields.io/badge/website-mediaharbor?style=for-the-badge&logo=github&label=MediaHarbor&link=https%3A%2F%2Fmediaharbor.github.io)

MediaHarbor is your all-in-one media downloader, supporting audio and video downloading / streaming from platforms like YouTube and more—all through an intuitive, streamlined interface.

## Installation
You can download and install MediaHarbor from releases page for your OS
##### It's recommended to use Windows Store for Windows users.

## Table of Contents
- [Features](#features)
- [Known Issues](#known-issues)
- [Installation](#installation)
- [Usage](#usage)
- [Contributing](#contributing)
- [License](#license)

## Features
- Download audio & video from popular platforms
- Seamless integration with yt-dlp
- Built-in search across YouTube, Spotify, and more
- Editing configuration files in app

## Known Issues

- Batch downloads can flicker until they finish.
- Play buttons on search page are limited to Videos, Tracks, and Podcasts only.
- Can't handle all errors on Downloading.

## Building

### Prerequisites
- Node.js
- npm

### Clone the Repository

```bash
git clone https://github.com/MediaHarbor/mediaharbor.git
cd mediaharbor
```

### Install Dependencies

```bash
npm install
```


## Usage

To start the application, run:

```bash
npm run start
```

To build the application for production:

```bash
npm run build
```

## Contributing
All contributions are welcome! Please read the contributing guidelines in the [CONTRIBUTING.md](CONTRIBUTING.md) file.

## License
This project is licensed under the GPL 3.0 - see the [LICENSE](LICENSE) file for details.