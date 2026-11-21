#!/usr/bin/env python3
"""Generate the Phase 05 fixture corpus for mochy-innertube.

The fixtures are SYNTHETIC: structurally faithful minimal payloads
hand-built from the upstream parser expectations (pin bde01cb4),
containing no user data. Building them with json.dumps guarantees
validity; tests/fixtures/README.md states provenance.
"""

import json
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "mochy-innertube", "tests", "fixtures")


def artist_run(name, browse_id, page_type="MUSIC_PAGE_TYPE_ARTIST"):
    return {
        "text": name,
        "navigationEndpoint": {
            "browseEndpoint": {
                "browseId": browse_id,
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": {"pageType": page_type}
                },
            }
        },
    }


def sep():
    return {"text": " • "}


def plain_run(text):
    return {"text": text}


def watch_endpoint(video_id, music_video_type=None, playlist_id=None):
    endpoint = {"watchEndpoint": {"videoId": video_id}}
    if music_video_type:
        endpoint["watchEndpoint"]["watchEndpointMusicSupportedConfigs"] = {
            "watchEndpointMusicConfig": {"musicVideoType": music_video_type}
        }
    if playlist_id:
        endpoint["watchEndpoint"]["playlistId"] = playlist_id
    return endpoint


def browse_endpoint(browse_id, page_type=None, params=None):
    endpoint = {"browseEndpoint": {"browseId": browse_id}}
    if page_type:
        endpoint["browseEndpoint"]["browseEndpointContextSupportedConfigs"] = {
            "browseEndpointContextMusicConfig": {"pageType": page_type}
        }
    if params:
        endpoint["browseEndpoint"]["params"] = params
    return endpoint


def watch_playlist(playlist_id, video_id=None):
    endpoint = {"watchPlaylistEndpoint": {"playlistId": playlist_id}}
    if video_id:
        endpoint["watchPlaylistEndpoint"]["videoId"] = video_id
    return endpoint


def thumbnails(video_id, size=120):
    return {
        "musicThumbnailRenderer": {
            "thumbnail": {
                "thumbnails": [
                    {
                        "url": f"https://i.ytimg.com/vi/{video_id}/{size}.jpg",
                        "width": size,
                        "height": size,
                    }
                ]
            }
        }
    }


def flex(runs):
    return {
        "musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}
    }


def fixed(runs):
    return {
        "musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": runs}}
    }


def badge(icon):
    return {"musicInlineBadgeRenderer": {"icon": {"iconType": icon}}}


def menu_toggle(default_icon, default_token, toggled_token):
    return {
        "toggleMenuServiceItemRenderer": {
            "defaultIcon": {"iconType": default_icon},
            "defaultServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": default_token}},
            "toggledServiceEndpoint": {"feedbackEndpoint": {"feedbackToken": toggled_token}},
        }
    }


def menu_nav(icon, endpoint):
    return {
        "menuNavigationItemRenderer": {
            "text": {"runs": [{"text": icon.title()}]},
            "icon": {"iconType": icon},
            "navigationEndpoint": endpoint,
        }
    }


def library_add_menu(add="ADD_SONG1", remove="REMOVE_SONG1"):
    return {"menuRenderer": {"items": [menu_toggle("LIBRARY_ADD", add, remove)]}}


def music_queue_menu(add="ADD_SONG5", remove="REMOVE_SONG5"):
    return {"menuRenderer": {"items": [menu_toggle("LIBRARY_SAVED", remove, add)]}}


def shuffle_mix_menu(shuffle_pl, radio_pl):
    return {
        "menuRenderer": {
            "items": [
                menu_nav("MUSIC_SHUFFLE", watch_playlist(shuffle_pl)),
                menu_nav("MIX", watch_playlist(radio_pl)),
            ]
        }
    }


def two_row_album(browse_id, playlist_id, title, artist, artist_id, year, thumb):
    return {
        "title": {"runs": [{"text": title}]},
        "subtitle": {
            "runs": [
                plain_run("Album"),
                sep(),
                artist_run(artist, artist_id),
                sep(),
                plain_run(str(year)),
            ]
        },
        "thumbnailRenderer": thumbnails(thumb),
        "thumbnailOverlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist(playlist_id)
                    }
                }
            }
        },
        "navigationEndpoint": browse_endpoint(browse_id, "MUSIC_PAGE_TYPE_ALBUM"),
    }


def song_flex_title(title, video_id):
    return flex([{"text": title, "navigationEndpoint": watch_endpoint(video_id, "MUSIC_VIDEO_TYPE_ATV")}])


def write(name, payload):
    path = os.path.join(OUT, f"{name}.json")
    with open(path, "w") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")
    print("wrote", path)


# ---------------------------------------------------------------------------
# search.json
# ---------------------------------------------------------------------------


def search_fixture():
    shelf_song = {
        "badges": [badge("MUSIC_EXPLICIT_BADGE")],
        "flexColumns": [
            song_flex_title("Song One", "SONG1"),
            flex([
                artist_run("Artist A", "UCA1"),
                sep(),
                artist_run("Album A", "MPREb_albumA", "MUSIC_PAGE_TYPE_ALBUM"),
                sep(),
                plain_run("3:45"),
            ]),
        ],
        "fixedColumns": [fixed([plain_run("3:45")])],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": "SET1", "videoId": "SONG1"},
        "menu": library_add_menu(),
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_endpoint("SONG1", "MUSIC_VIDEO_TYPE_ATV")
                    }
                }
            }
        },
        "navigationEndpoint": watch_endpoint("SONG1"),
    }
    shelf_artist = {
        "flexColumns": [
            flex([plain_run("Artist B")]),
            flex([plain_run("Artist"), sep(), plain_run("1.2M subscribers")]),
        ],
        "thumbnail": thumbnails("ARTB"),
        "menu": shuffle_mix_menu("RDAEM_ARTB", "RDM_ARTB"),
        "navigationEndpoint": browse_endpoint("UCB2", "MUSIC_PAGE_TYPE_ARTIST"),
    }
    item_section_album = {
        "flexColumns": [
            flex([plain_run("Album C")]),
            flex([
                plain_run("Album"),
                sep(),
                artist_run("Artist C", "UCC3"),
                sep(),
                plain_run("2021"),
            ]),
        ],
        "thumbnail": thumbnails("ALBC"),
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist("OLAK5uy_albumC")
                    }
                }
            }
        },
        "navigationEndpoint": browse_endpoint("MPREb_albumC", "MUSIC_PAGE_TYPE_ALBUM"),
    }
    # a playlist in the flat item section — the 2026-11-21 audit's
    # regression case for search_summary's "Playlists" group (the flat
    # path must parse through SearchSummaryPage, which keeps playlists)
    item_section_playlist = {
        "flexColumns": [
            flex([plain_run("Chill Mix")]),
            flex([
                plain_run("Playlist"),
                sep(),
                artist_run("Artist C", "UCC3"),
                sep(),
                plain_run("50 songs"),
            ]),
        ],
        "thumbnail": thumbnails("PLC"),
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist("PLchillmix")
                    }
                }
            }
        },
        "navigationEndpoint": browse_endpoint("VLPLchillmix", "MUSIC_PAGE_TYPE_PLAYLIST"),
    }
    card_shelf_song = {
        "flexColumns": [
            song_flex_title("Song One", "SONG1"),
            flex([
                plain_run("Song"),
                sep(),
                artist_run("Artist A", "UCA1"),
                sep(),
                plain_run("3:45"),
            ]),
        ],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG1"},
        "navigationEndpoint": watch_endpoint("SONG1"),
    }
    card = {
        "title": {"runs": [plain_run("Song One")]},
        "subtitle": {
            "runs": [
                plain_run("Song"),
                sep(),
                artist_run("Artist A", "UCA1"),
                sep(),
                plain_run("3:45"),
            ]
        },
        "thumbnail": thumbnails("SONG1", 544),
        "header": {
            "musicCardShelfHeaderBasicRenderer": {"title": {"runs": [plain_run("Top result")]}}
        },
        "contents": [{"musicResponsiveListItemRenderer": card_shelf_song}],
        "buttons": [
            {
                "buttonRenderer": {
                    "text": {"runs": [plain_run("Play")]},
                    "icon": {"iconType": "PLAY_ARROW"},
                    "command": watch_playlist("RDSONG1"),
                }
            }
        ],
        "onTap": watch_endpoint("SONG1", "MUSIC_VIDEO_TYPE_ATV"),
        "subtitleBadges": [badge("MUSIC_EXPLICIT_BADGE")],
    }
    continuation_playlist = {
        "flexColumns": [
            flex([plain_run("Playlist One")]),
            flex([
                plain_run("Playlist"),
                sep(),
                artist_run("Owner O", "UCO1", "MUSIC_PAGE_TYPE_USER_CHANNEL"),
                sep(),
                plain_run("50 songs"),
            ]),
        ],
        "thumbnail": thumbnails("PL1"),
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist("PL_playlist1")
                    }
                }
            }
        },
        "menu": shuffle_mix_menu("PL_playlist1", "RD_playlist1"),
        "navigationEndpoint": browse_endpoint("VLPL_playlist1", "MUSIC_PAGE_TYPE_PLAYLIST"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "tabbedSearchResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Search",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {"musicCardShelfRenderer": card},
                                        {
                                            "musicShelfRenderer": {
                                                "title": {"runs": [plain_run("Songs")]},
                                                "contents": [
                                                    {"musicResponsiveListItemRenderer": shelf_song},
                                                    {"musicResponsiveListItemRenderer": shelf_artist},
                                                ],
                                                "continuations": [
                                                    {"nextContinuationData": {"continuation": "SEARCH_CONT_NEXT"}}
                                                ],
                                            }
                                        },
                                        {
                                            "itemSectionRenderer": {
                                                "contents": [
                                                    {"musicResponsiveListItemRenderer": item_section_album},
                                                    {"musicResponsiveListItemRenderer": item_section_playlist},
                                                ]
                                            }
                                        },
                                    ]
                                }
                            },
                        }
                    }
                ]
            }
        },
        "continuationContents": {
            "musicShelfContinuation": {
                "contents": [{"musicResponsiveListItemRenderer": continuation_playlist}],
                "continuations": [
                    {"nextContinuationData": {"continuation": "SEARCH_CONT_2"}}
                ],
            }
        },
    }


# ---------------------------------------------------------------------------
# search_suggestions.json
# ---------------------------------------------------------------------------


def search_suggestions_fixture():
    recommended = {
        "flexColumns": [
            flex([plain_run("Song One")]),
            flex([
                plain_run("Songs"),
                sep(),
                artist_run("Artist A", "UCA1"),
                sep(),
                plain_run("3:45"),
            ]),
            flex([artist_run("Album A", "MPREb_albumA", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG1"},
        "navigationEndpoint": watch_endpoint("SONG1"),
    }
    return {
        "contents": [
            {
                "searchSuggestionsSectionRenderer": {
                    "contents": [
                        {
                            "searchSuggestionRenderer": {
                                "suggestion": {"runs": [plain_run("artist a")]},
                                "navigationEndpoint": {"searchEndpoint": {"query": "artist a"}},
                            }
                        },
                        {
                            "searchSuggestionRenderer": {
                                "suggestion": {"runs": [plain_run("artist a songs")]},
                                "navigationEndpoint": {"searchEndpoint": {"query": "artist a songs"}},
                            }
                        },
                    ]
                }
            },
            {
                "searchSuggestionsSectionRenderer": {
                    "contents": [{"musicResponsiveListItemRenderer": recommended}]
                }
            },
        ]
    }


# ---------------------------------------------------------------------------
# home.json
# ---------------------------------------------------------------------------


def home_fixture():
    quick_pick_song = {
        "title": {"runs": [plain_run("Song One")]},
        "subtitle": {"runs": [artist_run("Artist A", "UCA1"), sep(), plain_run("3:45")]},
        "thumbnailRenderer": thumbnails("SONG1"),
        "navigationEndpoint": watch_endpoint("SONG1"),
    }
    quick_pick_album = {
        "title": {"runs": [plain_run("Album A")]},
        "subtitle": {
            "runs": [
                plain_run("Album"),
                sep(),
                artist_run("Artist A", "UCA1"),
                sep(),
                plain_run("2021"),
            ]
        },
        "subtitleBadges": [badge("MUSIC_EXPLICIT_BADGE")],
        "thumbnailRenderer": thumbnails("ALBA"),
        "thumbnailOverlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist("OLAK5uy_albumA")
                    }
                }
            }
        },
        "navigationEndpoint": browse_endpoint("MPREb_albumA", "MUSIC_PAGE_TYPE_ALBUM"),
    }
    speed_dial_artist = {
        "title": {"runs": [plain_run("Artist B")]},
        "subtitle": {"runs": [plain_run("Artist")]},
        "thumbnailRenderer": thumbnails("ARTB"),
        "navigationEndpoint": browse_endpoint("UCB2", "MUSIC_PAGE_TYPE_ARTIST"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Home",
                            "content": {
                                "sectionListRenderer": {
                                    "header": {
                                        "chipCloudRenderer": {
                                            "chips": [
                                                {
                                                    "chipCloudChipRenderer": {
                                                        "isSelected": True,
                                                        "navigationEndpoint": browse_endpoint("FEmusic_home"),
                                                        "text": {"runs": [plain_run("Listen now")]},
                                                    }
                                                },
                                                {
                                                    "chipCloudChipRenderer": {
                                                        "isSelected": False,
                                                        "navigationEndpoint": browse_endpoint("FEmusic_home", params="chip_params_1"),
                                                        "onDeselectedCommand": browse_endpoint("FEmusic_home"),
                                                        "text": {"runs": [plain_run("Podcasts")]},
                                                    }
                                                },
                                            ]
                                        }
                                    },
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "strapline": {"runs": [plain_run("Because you like Artist A")]},
                                                        "title": {"runs": [plain_run("Quick picks")]},
                                                    }
                                                },
                                                "contents": [
                                                    {"musicTwoRowItemRenderer": quick_pick_song},
                                                    {"musicTwoRowItemRenderer": quick_pick_album},
                                                ],
                                                "itemSize": "MUSIC_TWO_ROW_ITEM_RENDERER_CAROUSEL",
                                                "numItemsPerColumn": 2,
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [plain_run("Speed dial")]},
                                                        "thumbnail": thumbnails("DIAL", 48),
                                                    }
                                                },
                                                "contents": [{"musicTwoRowItemRenderer": speed_dial_artist}],
                                                "numItemsPerColumn": 3,
                                                "itemSize": "MUSIC_TWO_ROW_ITEM_RENDERER_CAROUSEL",
                                            }
                                        },
                                    ],
                                    "continuations": [
                                        {"nextContinuationData": {"continuation": "HOME_CONT_NEXT"}}
                                    ],
                                }
                            },
                        }
                    }
                ]
            }
        },
    }


# ---------------------------------------------------------------------------
# album.json
# ---------------------------------------------------------------------------


def album_fixture():
    shelf_song = {
        "flexColumns": [
            song_flex_title("Song One", "SONG1"),
            flex([artist_run("Artist A", "UCA1"), sep(), plain_run("3:45")]),
            flex([artist_run("Album A", "MPREb_albumA", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "fixedColumns": [fixed([plain_run("3:45")])],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG1"},
        "menu": library_add_menu(),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "microformat": {
            "microformatDataRenderer": {
                "urlCanonical": "https://music.youtube.com/playlist?list=OLAK5uy_albumA"
            }
        },
        "background": thumbnails("ALBA", 544),
        "contents": {
            "twoColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Album",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicResponsiveHeaderRenderer": {
                                                "thumbnail": thumbnails("ALBA", 544),
                                                "buttons": [],
                                                "title": {"runs": [plain_run("Album A")]},
                                                "subtitle": {
                                                    "runs": [
                                                        plain_run("Album"),
                                                        sep(),
                                                        plain_run("10 songs"),
                                                        sep(),
                                                        plain_run("42 minutes"),
                                                        sep(),
                                                        plain_run("2021"),
                                                    ]
                                                },
                                                "secondSubtitle": {"runs": [plain_run("10 songs • 42 minutes")]},
                                                "straplineTextOne": {"runs": [artist_run("Artist A", "UCA1")]},
                                                "description": {"runs": [plain_run("The first album description.")]},
                                            }
                                        },
                                        {
                                            "musicDescriptionShelfRenderer": {
                                                "header": {"runs": [plain_run("About")]},
                                                "description": {"runs": [plain_run("The description of Album A.")]},
                                            }
                                        },
                                    ]
                                }
                            },
                        }
                    }
                ],
                "secondaryContents": {
                    "sectionListRenderer": {
                        "contents": [
                            {
                                "musicPlaylistShelfRenderer": {
                                    "playlistId": "OLAK5uy_albumA",
                                    "contents": [
                                        {"musicResponsiveListItemRenderer": shelf_song},
                                        {
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": {"token": "ALBUM_SONGS_CONT"}
                                                }
                                            }
                                        },
                                    ],
                                }
                            },
                            {
                                "musicCarouselShelfRenderer": {
                                    "header": {
                                        "musicCarouselShelfBasicHeaderRenderer": {
                                            "title": {"runs": [plain_run("Other versions")]}
                                        }
                                    },
                                    "contents": [
                                        {
                                            "musicTwoRowItemRenderer": two_row_album(
                                                "MPREb_albumD", "OLAK5uy_albumD", "Album A (Deluxe)",
                                                "Artist A", "UCA1", 2022, "ALBD",
                                            )
                                        }
                                    ],
                                }
                            },
                            {
                                "musicCarouselShelfRenderer": {
                                    "header": {
                                        "musicCarouselShelfBasicHeaderRenderer": {
                                            "title": {"runs": [plain_run("More from this artist")]}
                                        }
                                    },
                                    "contents": [
                                        {
                                            "musicTwoRowItemRenderer": two_row_album(
                                                "MPREb_albumE", "OLAK5uy_albumE", "Album E",
                                                "Artist A", "UCA1", 2023, "ALBE",
                                            )
                                        }
                                    ],
                                }
                            },
                        ]
                    }
                },
            }
        },
    }


# ---------------------------------------------------------------------------
# artist.json
# ---------------------------------------------------------------------------


def artist_fixture():
    shelf_song = {
        "flexColumns": [
            flex([plain_run("Song Two")]),
            flex([artist_run("Artist B", "UCB2"), sep(), plain_run("4:00")]),
            flex([artist_run("Album B", "MPREb_albumB", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "thumbnail": thumbnails("SONG2"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG2"},
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {"playNavigationEndpoint": watch_endpoint("SONG2")}
                }
            }
        },
        "navigationEndpoint": watch_endpoint("SONG2"),
    }
    singles_song = {
        "flexColumns": [
            flex([plain_run("Song Three")]),
            flex([artist_run("Artist B", "UCB2")]),
        ],
        "thumbnail": thumbnails("SONG3"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG3"},
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "header": {
            "musicImmersiveHeaderRenderer": {
                "title": {"runs": [plain_run("Artist B")]},
                "description": {"runs": [plain_run("The description of Artist B.")]},
                "thumbnail": thumbnails("ARTB", 544),
                "playButton": {
                    "buttonRenderer": {
                        "text": {"runs": [plain_run("Play")]},
                        "navigationEndpoint": watch_endpoint("SONG9", playlist_id="RDAO_ARTB"),
                    }
                },
                "startRadioButton": {
                    "buttonRenderer": {
                        "text": {"runs": [plain_run("Radio")]},
                        "navigationEndpoint": watch_endpoint("SONG9", playlist_id="RDEM_ARTB"),
                    }
                },
                "subscriptionButton": {
                    "subscribeButtonRenderer": {
                        "subscribed": True,
                        "channelId": "UCB2",
                        "longSubscriberCountText": {"runs": [plain_run("1.2M subscribers")]},
                    }
                },
                "menu": {"menuRenderer": {"items": []}},
                "monthlyListenerCount": {"runs": [plain_run("3,456,789 monthly listeners")]},
            }
        },
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Artist B",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicShelfRenderer": {
                                                "title": {
                                                    "runs": [
                                                        {
                                                            "text": "Songs",
                                                            "navigationEndpoint": browse_endpoint(
                                                                "UCB2", params="more_songs"
                                                            ),
                                                        }
                                                    ]
                                                },
                                                "contents": [{"musicResponsiveListItemRenderer": shelf_song}],
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [plain_run("Albums")]},
                                                        "moreContentButton": {
                                                            "buttonRenderer": {
                                                                "text": {"runs": [plain_run("More")]},
                                                                "navigationEndpoint": browse_endpoint("UCB2", params="albums"),
                                                            }
                                                        },
                                                    }
                                                },
                                                "contents": [
                                                    {
                                                        "musicTwoRowItemRenderer": two_row_album(
                                                            "MPREb_albumB", "OLAK5uy_albumB", "Album B",
                                                            "Artist B", "UCB2", 2022, "ALBB",
                                                        )
                                                    }
                                                ],
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [plain_run("Singles")]}
                                                    }
                                                },
                                                "contents": [{"musicResponsiveListItemRenderer": singles_song}],
                                            }
                                        },
                                        {
                                            "musicDescriptionShelfRenderer": {
                                                "header": {"runs": [plain_run("About")]},
                                                "description": {"runs": [plain_run("The description of Artist B.")]},
                                            }
                                        },
                                    ]
                                }
                            },
                        }
                    }
                ]
            }
        },
    }


# ---------------------------------------------------------------------------
# artist_items.json
# ---------------------------------------------------------------------------


def artist_items_fixture():
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "header": {"musicHeaderRenderer": {"title": {"runs": [plain_run("Albums")]}}},
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Albums",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {"title": {"runs": [plain_run("Albums")]}}
                                                },
                                                "items": [
                                                    {
                                                        "musicTwoRowItemRenderer": two_row_album(
                                                            "MPREb_albumB", "OLAK5uy_albumB", "Album B",
                                                            "Artist B", "UCB2", 2022, "ALBB",
                                                        )
                                                    }
                                                ],
                                                "continuations": [
                                                    {"nextContinuationData": {"continuation": "ARTIST_ITEMS_CONT"}}
                                                ],
                                            }
                                        }
                                    ]
                                }
                            },
                        }
                    }
                ]
            }
        },
        "continuationContents": {
            "gridContinuation": {
                "items": [
                    {
                        "musicTwoRowItemRenderer": two_row_album(
                            "MPREb_albumF", "OLAK5uy_albumF", "Album F",
                            "Artist B", "UCB2", 2024, "ALBF",
                        )
                    }
                ],
                "continuations": [
                    {"nextRadioContinuationData": {"continuation": "ARTIST_ITEMS_CONT_2"}}
                ],
            }
        },
    }


# ---------------------------------------------------------------------------
# playlist.json
# ---------------------------------------------------------------------------


def playlist_fixture():
    shelf_song = {
        "flexColumns": [
            flex([plain_run("Song Five")]),
            flex([artist_run("Owner O", "UCO1", "MUSIC_PAGE_TYPE_USER_CHANNEL"), sep(), plain_run("3:20")]),
            flex([artist_run("Album G", "MPREb_albumG", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "fixedColumns": [fixed([plain_run("3:20")])],
        "thumbnail": thumbnails("SONG5"),
        "playlistItemData": {"playlistSetVideoId": "SET_SONG5", "videoId": "SONG5"},
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {"playNavigationEndpoint": watch_endpoint("SONG5")}
                }
            }
        },
        "navigationEndpoint": watch_endpoint("SONG5"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "twoColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Playlist",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicResponsiveHeaderRenderer": {
                                                "thumbnail": thumbnails("PL1", 544),
                                                "buttons": [
                                                    {
                                                        "musicPlayButtonRenderer": {
                                                            "playNavigationEndpoint": watch_endpoint("SONG5", playlist_id="PL_playlist1")
                                                        }
                                                    },
                                                    {
                                                        "menuRenderer": {
                                                            "items": [
                                                                menu_nav("MUSIC_SHUFFLE", watch_playlist("PL_playlist1"))
                                                            ]
                                                        }
                                                    },
                                                    {
                                                        "musicPlayButtonRenderer": {
                                                            "playNavigationEndpoint": watch_endpoint("SONG5", playlist_id="PL_playlist1")
                                                        }
                                                    },
                                                    {
                                                        "menuRenderer": {
                                                            "items": [
                                                                menu_nav("MIX", watch_playlist("RD_playlist1"))
                                                            ]
                                                        }
                                                    },
                                                ],
                                                "title": {"runs": [plain_run("Playlist One")]},
                                                "subtitle": {
                                                    "runs": [plain_run("Playlist"), sep(), plain_run("Owner O")]
                                                },
                                                "secondSubtitle": {"runs": [plain_run("50 songs")]},
                                                "straplineTextOne": {
                                                    "runs": [artist_run("Owner O", "UCO1", "MUSIC_PAGE_TYPE_USER_CHANNEL")]
                                                },
                                            }
                                        }
                                    ]
                                }
                            },
                        }
                    }
                ],
                "secondaryContents": {
                    "sectionListRenderer": {
                        "contents": [
                            {
                                "musicPlaylistShelfRenderer": {
                                    "playlistId": "PL_playlist1",
                                    "contents": [
                                        {"musicResponsiveListItemRenderer": shelf_song},
                                        {
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": {"token": "PLAYLIST_SONGS_CONT"}
                                                }
                                            }
                                        },
                                    ],
                                    "continuations": [
                                        {"nextContinuationData": {"continuation": "PLAYLIST_SHELF_CONT"}}
                                    ],
                                }
                            },
                            {
                                "musicCarouselShelfRenderer": {
                                    "header": {
                                        "musicCarouselShelfBasicHeaderRenderer": {
                                            "title": {"runs": [plain_run("Related")]}
                                        }
                                    },
                                    "contents": [
                                        {
                                            "musicTwoRowItemRenderer": two_row_album(
                                                "MPREb_albumH", "OLAK5uy_albumH", "Album H",
                                                "Artist H", "UCH8", 2020, "ALBH",
                                            )
                                        }
                                    ],
                                }
                            },
                        ],
                        "continuations": [
                            {"nextContinuationData": {"continuation": "PLAYLIST_SECTION_CONT"}}
                        ],
                    }
                },
            }
        },
    }


# ---------------------------------------------------------------------------
# history.json
# ---------------------------------------------------------------------------


def history_fixture():
    today_song = {
        "flexColumns": [
            flex([plain_run("Song Five")]),
            flex([artist_run("Owner O", "UCO1", "MUSIC_PAGE_TYPE_USER_CHANNEL"), sep(), plain_run("3:20")]),
            flex([plain_run("Playlist One")]),
            flex([artist_run("Album G", "MPREb_albumG", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "fixedColumns": [fixed([plain_run("3:20")])],
        "thumbnail": thumbnails("SONG5"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG5"},
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {"playNavigationEndpoint": watch_endpoint("SONG5")}
                }
            }
        },
        "menu": {
            "menuRenderer": {
                "items": [
                    {
                        "menuServiceItemRenderer": {
                            "text": {"runs": [plain_run("Remove from history")]},
                            "icon": {"iconType": "REMOVE_FROM_HISTORY"},
                            "serviceEndpoint": {
                                "feedbackEndpoint": {"feedbackToken": "HISTORY_REMOVE_SONG5"}
                            },
                        }
                    },
                    menu_toggle("LIBRARY_ADD", "ADD_SONG5", "REMOVE_SONG5"),
                ]
            }
        },
    }
    yesterday_video = {
        "flexColumns": [
            flex([plain_run("Video Seven")]),
            flex([artist_run("Creator V", "UCV7", "MUSIC_PAGE_TYPE_USER_CHANNEL"), sep(), plain_run("1:16")]),
            flex([plain_run("Channel videos")]),
            flex([plain_run("Not an album")]),
        ],
        "fixedColumns": [fixed([plain_run("1:16")])],
        # a missing cover is no reason to lose the play (HistoryPage.kt:68-69)
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "VID7"},
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_endpoint("VID7", "MUSIC_VIDEO_TYPE_UGC")
                    }
                }
            }
        },
        "navigationEndpoint": watch_endpoint("VID7"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "History",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicShelfRenderer": {
                                                "title": {"runs": [plain_run("Today")]},
                                                "contents": [
                                                    {"musicResponsiveListItemRenderer": today_song},
                                                    {
                                                        "continuationItemRenderer": {
                                                            "continuationEndpoint": {
                                                                "continuationCommand": {"token": "HISTORY_SHELF_CONT"}
                                                            }
                                                        }
                                                    },
                                                ],
                                            }
                                        },
                                        {"musicShelfRenderer": {"contents": []}},
                                        {
                                            "musicShelfRenderer": {
                                                "title": {"runs": [plain_run("Yesterday")]},
                                                "contents": [{"musicResponsiveListItemRenderer": yesterday_video}],
                                            }
                                        },
                                    ],
                                    "continuations": [
                                        {"nextContinuationData": {"continuation": "HISTORY_LIST_CONT"}}
                                    ],
                                }
                            },
                        }
                    }
                ]
            }
        },
    }


# ---------------------------------------------------------------------------
# library.json
# ---------------------------------------------------------------------------


def library_fixture():
    grid_album = two_row_album("MPREb_albumA", "OLAK5uy_albumA", "Album A", "Artist A", "UCA1", 2021, "ALBA")
    grid_playlist = {
        "title": {"runs": [plain_run("My Mix")]},
        "subtitle": {
            "runs": [
                plain_run("Playlist"),
                sep(),
                plain_run("YouTube"),
                sep(),
                plain_run("25 songs"),
            ]
        },
        "thumbnailRenderer": thumbnails("MYMIX"),
        "thumbnailOverlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_playlist("RD_MIX1")
                    }
                }
            }
        },
        "menu": {
            "menuRenderer": {
                "items": [
                    menu_nav("MUSIC_SHUFFLE", watch_playlist("RD_MIX1")),
                    menu_nav("EDIT", browse_endpoint("VLRD_MIX1")),
                ]
            }
        },
        "navigationEndpoint": browse_endpoint("VLRD_MIX1", "MUSIC_PAGE_TYPE_PLAYLIST"),
    }
    shelf_song = {
        "flexColumns": [
            flex([plain_run("Song One")]),
            flex([artist_run("Artist A", "UCA1"), sep(), plain_run("3:45")]),
            flex([artist_run("Album A", "MPREb_albumA", "MUSIC_PAGE_TYPE_ALBUM")]),
        ],
        "fixedColumns": [fixed([plain_run("3:45")])],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG1"},
        "menu": music_queue_menu("ADD_SONG1", "REMOVE_SONG1"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Library",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {"title": {"runs": [plain_run("Albums")]}}
                                                },
                                                "items": [
                                                    {"musicTwoRowItemRenderer": grid_album},
                                                    {"musicTwoRowItemRenderer": grid_playlist},
                                                ],
                                            }
                                        }
                                    ]
                                }
                            },
                        }
                    }
                ]
            }
        },
        "continuationContents": {
            "musicShelfContinuation": {
                "contents": [{"musicResponsiveListItemRenderer": shelf_song}],
                "continuations": [
                    {"nextContinuationData": {"continuation": "LIBRARY_CONT_2"}}
                ],
            }
        },
    }


# ---------------------------------------------------------------------------
# related.json
# ---------------------------------------------------------------------------


def related_fixture():
    def song_item(video_id, title, artist, artist_id, with_album=True):
        columns = [
            flex([plain_run(title)]),
            flex([artist_run(artist, artist_id), sep(), plain_run("3:10")]),
        ]
        if with_album:
            columns.append(flex([artist_run("Album I", "MPREb_albumI", "MUSIC_PAGE_TYPE_ALBUM")]))
        return {
            "flexColumns": columns,
            "thumbnail": thumbnails(video_id),
            "playlistItemData": {"playlistSetVideoId": None, "videoId": video_id},
            "overlay": {
                "musicItemThumbnailOverlayRenderer": {
                    "content": {
                        "musicPlayButtonRenderer": {
                            "playNavigationEndpoint": watch_endpoint(video_id, "MUSIC_VIDEO_TYPE_ATV")
                        }
                    }
                }
            },
        }

    video_item = {
        "flexColumns": [
            flex([plain_run("Video Ten")]),
            flex([artist_run("Creator V", "UCV7", "MUSIC_PAGE_TYPE_USER_CHANNEL")]),
        ],
        "thumbnail": thumbnails("VID10"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "VID10"},
        "overlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {
                        "playNavigationEndpoint": watch_endpoint("VID10", "MUSIC_VIDEO_TYPE_UGC")
                    }
                }
            }
        },
    }
    carousel_playlist = {
        "title": {"runs": [plain_run("Mixtape N")]},
        "subtitle": {
            "runs": [
                plain_run("Owner N"),
                sep(),
                plain_run("Playlist"),
                sep(),
                plain_run("12 songs"),
            ]
        },
        "thumbnailRenderer": thumbnails("PLN"),
        "thumbnailOverlay": {
            "musicItemThumbnailOverlayRenderer": {
                "content": {
                    "musicPlayButtonRenderer": {"playNavigationEndpoint": watch_playlist("PL_mixN")}
                }
            }
        },
        "menu": {
            "menuRenderer": {
                "items": [menu_nav("MUSIC_SHUFFLE", watch_playlist("PL_mixN"))]
            }
        },
        "navigationEndpoint": browse_endpoint("VLPL_mixN", "MUSIC_PAGE_TYPE_PLAYLIST"),
    }
    carousel_artist = {
        "title": {"runs": [plain_run("Artist M")]},
        "subtitle": {"runs": [plain_run("Artist"), sep(), plain_run("890K subscribers")]},
        "thumbnailRenderer": thumbnails("ARTM"),
        "navigationEndpoint": browse_endpoint("UCM13", "MUSIC_PAGE_TYPE_ARTIST"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "sectionListRenderer": {
                "contents": [
                    {
                        "musicCarouselShelfRenderer": {
                            "header": {
                                "musicCarouselShelfBasicHeaderRenderer": {
                                    "title": {"runs": [plain_run("You might also like")]}
                                }
                            },
                            "contents": [
                                {"musicResponsiveListItemRenderer": song_item("SONG9", "Song Nine", "Artist I", "UCI9")},
                                {"musicResponsiveListItemRenderer": video_item},
                            ],
                        }
                    },
                    {
                        "musicShelfRenderer": {
                            "title": {"runs": [plain_run("More songs")]},
                            "contents": [
                                {"musicResponsiveListItemRenderer": song_item("SONG11", "Song Eleven", "Artist J", "UCJ10")}
                            ],
                        }
                    },
                    {
                        "itemSectionRenderer": {
                            "contents": [
                                {"musicResponsiveListItemRenderer": song_item("SONG12", "Song Twelve", "Artist K", "UCK11")}
                            ]
                        }
                    },
                    {
                        "musicCarouselShelfRenderer": {
                            "header": {
                                "musicCarouselShelfBasicHeaderRenderer": {
                                    "title": {"runs": [plain_run("Related albums")]}
                                }
                            },
                            "contents": [
                                {
                                    "musicTwoRowItemRenderer": two_row_album(
                                        "MPREb_albumL", "OLAK5uy_albumL", "Album L",
                                        "Artist L", "UCL12", 2019, "ALBL",
                                    )
                                },
                                {"musicTwoRowItemRenderer": carousel_artist},
                                {"musicTwoRowItemRenderer": carousel_playlist},
                            ],
                        }
                    },
                ]
            }
        },
    }


# ---------------------------------------------------------------------------
# charts.json
# ---------------------------------------------------------------------------


def charts_fixture():
    trending_song = {
        "flexColumns": [
            flex([plain_run("1"), plain_run("Song One")]),
            flex([artist_run("Artist A", "UCA1"), sep(), plain_run("Song")]),
            flex([plain_run("1"), plain_run(" ▲ 2")]),
        ],
        "thumbnail": thumbnails("SONG1"),
        "playlistItemData": {"playlistSetVideoId": None, "videoId": "SONG1"},
    }
    top_song = {
        "title": {"runs": [plain_run("Top Song")]},
        "subtitle": {"runs": [artist_run("Artist T", "UCT20"), sep(), plain_run("Song")]},
        "thumbnailRenderer": thumbnails("TOP20"),
        "navigationEndpoint": watch_endpoint("TOP20", "MUSIC_VIDEO_TYPE_ATV"),
    }
    return {
        "responseContext": {"visitorData": "<redacted:visitordata>"},
        "contents": {
            "singleColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Charts",
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [plain_run("Trending")]}
                                                    }
                                                },
                                                "contents": [{"musicResponsiveListItemRenderer": trending_song}],
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [plain_run("Top 50")]}
                                                    }
                                                },
                                                "contents": [{"musicTwoRowItemRenderer": top_song}],
                                            }
                                        },
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {"title": {"runs": [plain_run("New releases")]}}
                                                },
                                                "items": [
                                                    {
                                                        "musicTwoRowItemRenderer": two_row_album(
                                                            "MPREb_albumN", "OLAK5uy_albumN", "Album N",
                                                            "Artist N", "UCN14", 2026, "ALBN",
                                                        )
                                                    }
                                                ],
                                            }
                                        },
                                    ],
                                    "continuations": [
                                        {"nextContinuationData": {"continuation": "CHARTS_CONT"}}
                                    ],
                                }
                            },
                        }
                    }
                ]
            }
        },
        "continuationContents": {
            "sectionListContinuation": {
                "contents": [],
                "continuations": [
                    {"nextContinuationData": {"continuation": "CHARTS_CONT_2"}}
                ],
            }
        },
    }


if __name__ == "__main__":
    write("search", search_fixture())
    write("search_suggestions", search_suggestions_fixture())
    write("home", home_fixture())
    write("album", album_fixture())
    write("artist", artist_fixture())
    write("artist_items", artist_items_fixture())
    write("playlist", playlist_fixture())
    write("history", history_fixture())
    write("library", library_fixture())
    write("related", related_fixture())
    write("charts", charts_fixture())
