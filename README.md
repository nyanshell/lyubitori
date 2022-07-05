# lyubitori
A simple tool for collecting favorited images from twitter. Support Twitter API v1.1.

## Build

```bash
> cargo build --release
```

## Usage

### Download recent favorited

```bash
> APP_CLIENT_KEY=<api-key> APP_CLIENT_SECRET=<api-secret-key> RESOURCE_OWNER_KEY=<access-token> RESOURCE_OWNER_SECRET=<access-secret> ./lyubitori download --save-path <image-save-path>
```

### Download all history

```bash
APP_CLIENT_KEY=<api-key> APP_CLIENT_SECRET=<api-secret-key> RESOURCE_OWNER_KEY=<access-token> RESOURCE_OWNER_SECRET=<access-secret> ./lyubitori download --save-path <image-save-path> --scanall
```

## TODO

- ~~oauth: user auth for fetching protected tweets~~[DONE]
- download video/mp4
- ~~download png img~~[DONE]
- like & download tweets images from certain user's DM
- send update messages to slack if new images were added
- update image meta data into elasticsearch
- call image classification API to tag images & add results to elasticsearch
- ~~download media content from protected user using cookies?~~[too hard to implement]
- Github Actions
