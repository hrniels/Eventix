#!/usr/bin/env python3

__license__ = 'MIT'
import aiohttp
import argparse
import asyncio
import json
import hashlib
import logging
import re
import os

arches = {
        'linux-x86_64': 'x86_64',
        'linux-x86_32': 'i386',
        'linux-aarch_64': 'aarch64',
        'linux-aarch_32': 'arm'
}

MAX_CONCURRENT_DOWNLOADS = 8

async def get_remote_sha256(http_session, url):
    logging.info(f"started sha256({url})")
    sha256 = hashlib.sha256()
    async with http_session.get(url) as response:
        if response.status != 200:
            raise Exception(f"Failed to download {url}: {response.status}")
        while True:
            data = await response.content.read(4096)
            if not data:
                break
            sha256.update(data)
    logging.info(f"done sha256({url})")
    return sha256.hexdigest()

def get_file_sha256(path):
    sha256 = hashlib.sha256()
    with open(path, 'rb') as file:
        while data := file.read(4096):
            sha256.update(data)
    return sha256.hexdigest()

async def parse_url(http_session, url, destdir, arch=None, maven_repo=None):
    # Extract path from URL to mirror Maven repository layout
    # e.g., https://repo.maven.apache.org/maven2/org/apache/maven/plugins/maven-resources-plugin/3.4.0/maven-resources-plugin-3.4.0.pom
    # results in org/apache/maven/plugins/maven-resources-plugin/3.4.0/
    path_match = re.search(r'/maven2/(.+)/([^/]+)$', url)
    if path_match:
        sub_dest = os.path.join(destdir, path_match.group(1))
    else:
        sub_dest = destdir

    if maven_repo and path_match:
        local_path = os.path.join(maven_repo, path_match.group(1), path_match.group(2))
        if os.path.isfile(local_path):
            sha256 = get_file_sha256(local_path)
        else:
            logging.warning(
                f"{url} not found in local Maven repo {maven_repo}, computing sha256 remotely"
            )
            sha256 = await get_remote_sha256(http_session, url)
    else:
        sha256 = await get_remote_sha256(http_session, url)

    ret = [{ 'type': 'file',
            'url': url,
            'sha256': sha256,
            'dest': sub_dest, }]
    if arch:
        ret[0]['only-arches'] = [arch]
    return ret

def arch_for_url(url, urls_arch):
    arch = None
    try:
        arch = urls_arch[url]
    except KeyError:
        pass
    return arch

async def parse_urls(urls, urls_arch, destdir, maven_repo=None):
    sources = []
    sha_coros = []
    connector = aiohttp.TCPConnector(limit=MAX_CONCURRENT_DOWNLOADS)
    async with aiohttp.ClientSession(connector=connector) as http_session:
        for url in dict.fromkeys(urls):
            arch = arch_for_url(url, urls_arch)
            sha_coros.append(parse_url(http_session, str(url), destdir, arch, maven_repo))
        sources.extend(sum(await asyncio.gather(*sha_coros), []))
    return sources

def gradle_arch_to_flatpak_arch(arch):
    return arches[arch]

def flatpak_arch_to_gradle_arch(arch):
    rev_arches = dict((v, k) for k, v in arches.items())
    return rev_arches[arch]

def main():
    logging.basicConfig(
        level=os.environ.get('LOGLEVEL', 'WARNING').upper()
    )
    parser = argparse.ArgumentParser()
    parser.add_argument('input', help='The gradle log file')
    parser.add_argument('output', help='The output JSON sources file')
    parser.add_argument('--destdir',
                        help='The directory the generated sources file will save sources to',
                        default='dependencies')
    parser.add_argument('--arches',
                        help='Comma-separated list of architectures the generated sources will be for',
                        default='x86_64,aarch64,i386,arm')
    parser.add_argument('--maven-repo',
                        help='Local Maven repository to use for calculating checksums')
    args = parser.parse_args()
    req_flatpak_arches = args.arches.split(',')
    req_gradle_arches = []
    for arch in req_flatpak_arches:
        req_gradle_arches.append(flatpak_arch_to_gradle_arch(arch))

    urls = []
    urls_arch = {}
    r = re.compile('https://[\\w/\\-?=%.]+\\.[\\w/\\-?=%.]+')
    with open(args.input,'r') as f:
        for lines in f:
            res = r.findall(lines)
            for url in res:
                if url.endswith('.jar') or url.endswith('.pom'):
                    urls.append(url)
                elif url.endswith('.exe'):
                    for host in req_gradle_arches:
                        if host in url:
                            for arch in req_gradle_arches:
                                new_url = url.replace(host, arch)
                                urls.append(new_url)
                                urls_arch[new_url] = gradle_arch_to_flatpak_arch(arch)

    # print(urls)
    # print(urls_arch)

    sources = asyncio.run(parse_urls(urls, urls_arch, args.destdir, args.maven_repo))

    sources.sort(key=lambda x: x['url'])

    with open(args.output, 'w') as fp:
        json.dump(sources, fp, indent=4)
        fp.write('\n')


if __name__ == '__main__':
    main()
