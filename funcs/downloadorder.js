let downloadCount = 0;

const getNextDownloadOrder = () => {
    return ++downloadCount;
};

module.exports = { getNextDownloadOrder };