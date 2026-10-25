use katsanto_db::{
    Db, DbError, challenges, invites, now, passkeys, recovery, resets, sessions, settings,
    users::{self, NewUser},
};

async fn user(db: &Db, name: &str) -> users::User {
    users::create(
        db.pool(),
        NewUser {
            username: name,
            webauthn_id: name.as_bytes(),
            password_hash: None,
            is_admin: false,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn usernames_are_unique_case_insensitively() {
    let db = Db::in_memory().await.unwrap();
    user(&db, "alice").await;
    let err = users::create(
        db.pool(),
        NewUser {
            username: "ALICE",
            webauthn_id: b"other",
            password_hash: None,
            is_admin: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DbError::Conflict("username")));
    assert_eq!(
        users::by_username(db.pool(), "Alice")
            .await
            .unwrap()
            .unwrap()
            .username,
        "alice"
    );
}

#[tokio::test]
async fn invites_can_be_used_once() {
    let db = Db::in_memory().await.unwrap();
    let admin = user(&db, "admin").await;
    let bob = user(&db, "bob").await;
    let inv = invites::create(db.pool(), b"hash", "for bob", admin.id, now() + 60)
        .await
        .unwrap();
    assert!(
        invites::find_valid(db.pool(), b"hash")
            .await
            .unwrap()
            .is_some()
    );
    assert!(invites::consume(db.pool(), inv.id, bob.id).await.unwrap());
    assert!(!invites::consume(db.pool(), inv.id, bob.id).await.unwrap());
    assert!(
        invites::find_valid(db.pool(), b"hash")
            .await
            .unwrap()
            .is_none()
    );

    invites::create(db.pool(), b"expired", "", admin.id, now() - 1)
        .await
        .unwrap();
    assert!(
        invites::find_valid(db.pool(), b"expired")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sessions_of_disabled_users_are_inactive() {
    let db = Db::in_memory().await.unwrap();
    let u = user(&db, "carol").await;
    let s = sessions::create(db.pool(), b"tok", u.id, "test", 3600)
        .await
        .unwrap();
    assert_eq!(
        sessions::find_active(db.pool(), b"tok")
            .await
            .unwrap()
            .unwrap()
            .id,
        s.id
    );

    users::set_disabled(db.pool(), u.id, true).await.unwrap();
    assert!(
        sessions::find_active(db.pool(), b"tok")
            .await
            .unwrap()
            .is_none()
    );

    users::set_disabled(db.pool(), u.id, false).await.unwrap();
    sessions::create(db.pool(), b"old", u.id, "test", -1)
        .await
        .unwrap();
    assert!(
        sessions::find_active(db.pool(), b"old")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sign_out_everywhere_keeps_current_session() {
    let db = Db::in_memory().await.unwrap();
    let u = user(&db, "dave").await;
    let current = sessions::create(db.pool(), b"a", u.id, "", 3600)
        .await
        .unwrap();
    sessions::create(db.pool(), b"b", u.id, "", 3600)
        .await
        .unwrap();
    sessions::create(db.pool(), b"c", u.id, "", 3600)
        .await
        .unwrap();
    assert_eq!(
        sessions::delete_all(db.pool(), u.id, Some(current.id))
            .await
            .unwrap(),
        2
    );
    assert_eq!(sessions::for_user(db.pool(), u.id).await.unwrap().len(), 1);
    assert_eq!(
        sessions::delete_all(db.pool(), u.id, None).await.unwrap(),
        1
    );
}

#[tokio::test]
async fn challenges_are_single_use() {
    let db = Db::in_memory().await.unwrap();
    challenges::put(db.pool(), "id1", "login", "{}", 60)
        .await
        .unwrap();
    assert!(
        challenges::take(db.pool(), "id1", "register")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        challenges::take(db.pool(), "id1", "login")
            .await
            .unwrap()
            .as_deref(),
        Some("{}")
    );
    assert!(
        challenges::take(db.pool(), "id1", "login")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn recovery_codes_are_single_use_and_replaceable() {
    let db = Db::in_memory().await.unwrap();
    let u = user(&db, "erin").await;
    let mut tx = db.begin().await.unwrap();
    recovery::replace(&mut tx, u.id, &[b"c1".to_vec(), b"c2".to_vec()])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(recovery::consume(db.pool(), u.id, b"c1").await.unwrap());
    assert!(!recovery::consume(db.pool(), u.id, b"c1").await.unwrap());
    assert_eq!(recovery::remaining(db.pool(), u.id).await.unwrap(), 1);

    let mut tx = db.begin().await.unwrap();
    recovery::replace(&mut tx, u.id, &[b"c3".to_vec()])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!recovery::consume(db.pool(), u.id, b"c2").await.unwrap());
    assert!(recovery::consume(db.pool(), u.id, b"c3").await.unwrap());
}

#[tokio::test]
async fn new_reset_link_replaces_old_one() {
    let db = Db::in_memory().await.unwrap();
    let admin = user(&db, "admin").await;
    let u = user(&db, "frank").await;
    let mut tx = db.begin().await.unwrap();
    resets::create(&mut tx, b"r1", u.id, admin.id, now() + 60)
        .await
        .unwrap();
    let r2 = resets::create(&mut tx, b"r2", u.id, admin.id, now() + 60)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        resets::find_valid(db.pool(), b"r1")
            .await
            .unwrap()
            .is_none()
    );
    assert!(resets::consume(db.pool(), r2.id).await.unwrap());
    assert!(!resets::consume(db.pool(), r2.id).await.unwrap());
}

#[tokio::test]
async fn deleting_a_user_cascades() {
    let db = Db::in_memory().await.unwrap();
    let u = user(&db, "gina").await;
    passkeys::create(db.pool(), u.id, b"cred", "{}", "Phone")
        .await
        .unwrap();
    sessions::create(db.pool(), b"tok", u.id, "", 3600)
        .await
        .unwrap();
    users::delete(db.pool(), u.id).await.unwrap();
    assert_eq!(passkeys::count_for_user(db.pool(), u.id).await.unwrap(), 0);
    assert!(
        sessions::find_active(db.pool(), b"tok")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn passkeys_are_scoped_to_their_user() {
    let db = Db::in_memory().await.unwrap();
    let a = user(&db, "hank").await;
    let b = user(&db, "ivy").await;
    let pk = passkeys::create(db.pool(), a.id, b"cred", "{}", "Laptop")
        .await
        .unwrap();
    assert!(!passkeys::delete(db.pool(), b.id, pk.id).await.unwrap());
    assert!(!passkeys::rename(db.pool(), b.id, pk.id, "x").await.unwrap());
    assert!(
        passkeys::rename(db.pool(), a.id, pk.id, "Work laptop")
            .await
            .unwrap()
    );
    let err = passkeys::create(db.pool(), b.id, b"cred", "{}", "dup")
        .await
        .unwrap_err();
    assert!(matches!(err, DbError::Conflict("passkey")));
}

#[tokio::test]
async fn settings_roundtrip() {
    let db = Db::in_memory().await.unwrap();
    assert_eq!(
        settings::get::<String>(db.pool(), "mode").await.unwrap(),
        None
    );
    settings::set(db.pool(), "mode", &"invite").await.unwrap();
    settings::set(db.pool(), "mode", &"open").await.unwrap();
    assert_eq!(
        settings::get::<String>(db.pool(), "mode")
            .await
            .unwrap()
            .as_deref(),
        Some("open")
    );
}

mod watch {
    use katsanto_db::{
        Db, progress,
        users::{self, NewUser},
        videos::{self, ChannelMeta, VideoMeta},
    };

    async fn setup() -> (Db, i64) {
        let db = Db::in_memory().await.unwrap();
        let u = users::create(
            db.pool(),
            NewUser {
                username: "w",
                webauthn_id: b"w",
                password_hash: None,
                is_admin: false,
            },
        )
        .await
        .unwrap();
        (db, u.id)
    }

    async fn video(db: &Db, id: &str, duration: i64) {
        videos::upsert_channel(
            db.pool(),
            &ChannelMeta {
                id: "UC1",
                name: "Chan",
                handle: None,
                avatar_url: None,
                subscriber_count: None,
            },
        )
        .await
        .unwrap();
        videos::upsert_video(
            db.pool(),
            &VideoMeta {
                id,
                channel_id: "UC1",
                title: "T",
                thumbnail_url: None,
                duration_s: Some(duration),
                published_at: None,
                is_short: false,
                is_live: false,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sparse_upserts_keep_richer_data() {
        let (db, _) = setup().await;
        videos::upsert_channel(
            db.pool(),
            &ChannelMeta {
                id: "UC1",
                name: "Chan",
                handle: Some("@chan"),
                avatar_url: Some("a.jpg"),
                subscriber_count: Some(5),
            },
        )
        .await
        .unwrap();
        video(&db, "v1", 100).await; // channel upsert without avatar/handle
        let (avatar, handle): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT avatar_url, handle FROM channels WHERE id = 'UC1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(avatar.as_deref(), Some("a.jpg"));
        assert_eq!(handle.as_deref(), Some("@chan"));
        assert!(videos::exists(db.pool(), "v1").await.unwrap());
    }

    #[tokio::test]
    async fn progress_and_continue_watching() {
        let (db, user) = setup().await;
        for id in ["a", "b", "c"] {
            video(&db, id, 600).await;
        }
        progress::save(db.pool(), user, "a", 120.0, 600.0)
            .await
            .unwrap();
        progress::save(db.pool(), user, "b", 10.0, 600.0)
            .await
            .unwrap(); // just a peek
        let c = progress::save(db.pool(), user, "c", 590.0, 600.0)
            .await
            .unwrap();
        assert!(c.finished);

        let list = progress::continue_watching(db.pool(), user, 10)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].video.id, "a");
        assert_eq!(list[0].video.channel_name, "Chan");
        assert_eq!(list[0].position_s, 120.0);

        let many = progress::for_videos(db.pool(), user, &["a".into(), "c".into(), "zzz".into()])
            .await
            .unwrap();
        assert_eq!(many.len(), 2);

        // Rewatching a finished video resets it.
        let c = progress::save(db.pool(), user, "c", 60.0, 600.0)
            .await
            .unwrap();
        assert!(!c.finished);
        assert!(progress::delete(db.pool(), user, "a").await.unwrap());
        assert!(progress::get(db.pool(), user, "a").await.unwrap().is_none());
    }
}

mod subscriptions_and_feed {
    use katsanto_db::{
        Db, feed, now, progress, subscriptions,
        users::{self, NewUser},
        videos::{self, ChannelMeta},
    };

    async fn user(db: &Db, name: &str) -> i64 {
        users::create(
            db.pool(),
            NewUser {
                username: name,
                webauthn_id: name.as_bytes(),
                password_hash: None,
                is_admin: false,
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn channel(db: &Db, id: &str) {
        videos::upsert_channel(
            db.pool(),
            &ChannelMeta {
                id,
                name: id,
                handle: None,
                avatar_url: None,
                subscriber_count: None,
            },
        )
        .await
        .unwrap();
    }

    async fn rss_video(db: &Db, id: &str, channel: &str, published: i64, short: bool) -> bool {
        let mut conn = db.acquire().await.unwrap();
        feed::upsert_from_rss(
            &mut conn,
            &feed::FeedVideoMeta {
                id,
                channel_id: channel,
                title: id,
                thumbnail_url: None,
                published_at: published,
                view_count: Some(1),
                is_short: short,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe() {
        let db = Db::in_memory().await.unwrap();
        let u = user(&db, "u").await;
        channel(&db, "UC1").await;
        channel(&db, "UC2").await;
        assert!(subscriptions::subscribe(db.pool(), u, "UC1").await.unwrap());
        assert!(!subscriptions::subscribe(db.pool(), u, "UC1").await.unwrap());
        subscriptions::subscribe(db.pool(), u, "UC2").await.unwrap();
        assert_eq!(subscriptions::list(db.pool(), u).await.unwrap().len(), 2);
        let n = subscriptions::unsubscribe(db.pool(), u, &["UC1".into(), "UC9".into()])
            .await
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            subscriptions::channel_ids(db.pool(), u).await.unwrap(),
            vec!["UC2"]
        );
    }

    #[tokio::test]
    async fn feed_shows_subscribed_channels_newest_first() {
        let db = Db::in_memory().await.unwrap();
        let (a, b) = (user(&db, "a").await, user(&db, "b").await);
        for c in ["UC1", "UC2"] {
            channel(&db, c).await;
        }
        subscriptions::subscribe(db.pool(), a, "UC1").await.unwrap();
        subscriptions::subscribe(db.pool(), b, "UC2").await.unwrap();
        assert!(rss_video(&db, "v1", "UC1", 100, false).await);
        assert!(
            !rss_video(&db, "v1", "UC1", 100, false).await,
            "second time is not new"
        );
        rss_video(&db, "v2", "UC1", 300, false).await;
        rss_video(&db, "v3", "UC1", 200, true).await;
        rss_video(&db, "v4", "UC2", 400, false).await;

        let q = |include_shorts| feed::FeedQuery {
            include_shorts,
            include_live: true,
            limit: 10,
            ..Default::default()
        };
        let ids = |rows: Vec<feed::FeedRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(
            ids(feed::feed(db.pool(), a, &q(true)).await.unwrap()),
            ["v2", "v3", "v1"]
        );
        assert_eq!(
            ids(feed::feed(db.pool(), a, &q(false)).await.unwrap()),
            ["v2", "v1"]
        );

        // Cursor pagination.
        let page = feed::FeedQuery {
            after: Some((300, "v2".into())),
            limit: 1,
            ..q(true)
        };
        assert_eq!(ids(feed::feed(db.pool(), a, &page).await.unwrap()), ["v3"]);

        // Watched videos can be hidden; progress comes along.
        progress::save(db.pool(), a, "v2", 595.0, 600.0)
            .await
            .unwrap();
        let rows = feed::feed(db.pool(), a, &q(true)).await.unwrap();
        assert_eq!(rows[0].finished, Some(true));
        let unwatched = feed::FeedQuery {
            watched: feed::Watched::HideFinished,
            ..q(true)
        };
        assert_eq!(
            ids(feed::feed(db.pool(), a, &unwatched).await.unwrap()),
            ["v3", "v1"]
        );
    }

    #[tokio::test]
    async fn refresh_bookkeeping() {
        let db = Db::in_memory().await.unwrap();
        let u = user(&db, "u").await;
        channel(&db, "UC1").await;
        channel(&db, "UC2").await; // nobody subscribed: never polled
        subscriptions::subscribe(db.pool(), u, "UC1").await.unwrap();
        assert_eq!(
            feed::due_channels(db.pool(), 10).await.unwrap(),
            vec!["UC1"]
        );

        feed::mark_fetched(db.pool(), "UC1", Some(500), now() + 3600)
            .await
            .unwrap();
        assert!(feed::due_channels(db.pool(), 10).await.unwrap().is_empty());
        assert!(feed::next_due_at(db.pool()).await.unwrap().unwrap() > now());
        feed::mark_fetched(db.pool(), "UC1", Some(100), now() + 3600)
            .await
            .unwrap();
        let st = feed::refresh_state(db.pool(), "UC1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(st.last_video_at, Some(500), "keeps the newest");

        feed::mark_failed(db.pool(), "UC1", "boom", now() - 1)
            .await
            .unwrap();
        feed::mark_failed(db.pool(), "UC1", "boom", now() - 1)
            .await
            .unwrap();
        assert_eq!(
            feed::refresh_state(db.pool(), "UC1")
                .await
                .unwrap()
                .unwrap()
                .fetch_errors,
            2
        );
        assert_eq!(
            subscriptions::list(db.pool(), u).await.unwrap()[0]
                .fetch_error
                .as_deref(),
            Some("boom")
        );

        rss_video(&db, "v1", "UC1", 100, false).await;
        rss_video(&db, "v2", "UC1", 200, false).await;
        assert_eq!(feed::unenriched(db.pool(), "UC1").await.unwrap().len(), 2);
        feed::enrich(
            db.pool(),
            &feed::Enrichment {
                id: "v1",
                duration_s: Some(61),
                thumbnail_url: None,
                is_short: false,
                is_live: false,
            },
        )
        .await
        .unwrap();
        feed::mark_enriched(db.pool(), &["v2".into()])
            .await
            .unwrap();
        assert!(feed::unenriched(db.pool(), "UC1").await.unwrap().is_empty());
    }
}

mod feed_filters {
    use katsanto_db::{
        Db, feed, hidden, progress, subscriptions, tags,
        users::{self, NewUser},
        videos::{self, ChannelMeta},
    };

    async fn setup() -> (Db, i64) {
        let db = Db::in_memory().await.unwrap();
        let u = users::create(
            db.pool(),
            NewUser {
                username: "f",
                webauthn_id: b"f",
                password_hash: None,
                is_admin: false,
            },
        )
        .await
        .unwrap()
        .id;
        for c in ["UCa", "UCb", "UCc"] {
            videos::upsert_channel(
                db.pool(),
                &ChannelMeta {
                    id: c,
                    name: c,
                    handle: None,
                    avatar_url: None,
                    subscriber_count: None,
                },
            )
            .await
            .unwrap();
            subscriptions::subscribe(db.pool(), u, c).await.unwrap();
        }
        // (id, channel, published, duration, short, live)
        for (id, ch, at, dur, short, live) in [
            ("a1", "UCa", 100, Some(60), false, false),
            ("a2", "UCa", 200, Some(1200), false, false),
            ("b1", "UCb", 300, Some(30), true, false),
            ("b2", "UCb", 400, None, false, true),
            ("c1", "UCc", 500, Some(3000), false, false),
        ] {
            let mut conn = db.acquire().await.unwrap();
            feed::upsert_from_rss(
                &mut conn,
                &feed::FeedVideoMeta {
                    id,
                    channel_id: ch,
                    title: id,
                    thumbnail_url: None,
                    published_at: at,
                    view_count: None,
                    is_short: short,
                },
            )
            .await
            .unwrap();
            feed::enrich(
                &mut *conn,
                &feed::Enrichment {
                    id,
                    duration_s: dur,
                    thumbnail_url: None,
                    is_short: short,
                    is_live: live,
                },
            )
            .await
            .unwrap();
        }
        (db, u)
    }

    async fn ids(db: &Db, u: i64, q: feed::FeedQuery) -> Vec<String> {
        let q = feed::FeedQuery {
            limit: 50,
            include_shorts: true,
            include_live: true,
            ..q
        };
        feed::feed(db.pool(), u, &q)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    #[tokio::test]
    async fn tags_and_channels() {
        let (db, u) = setup().await;
        let prog = tags::create(db.pool(), u, "Programming", "blue")
            .await
            .unwrap();
        let mut conn = db.acquire().await.unwrap();
        tags::set_channel_tags(&mut conn, u, "UCa", &[prog.id])
            .await
            .unwrap();
        drop(conn);
        let by_tag = ids(
            &db,
            u,
            feed::FeedQuery {
                tag_ids: vec![prog.id],
                ..Default::default()
            },
        )
        .await;
        assert_eq!(by_tag, ["a2", "a1"]);
        let tag_or_channel = ids(
            &db,
            u,
            feed::FeedQuery {
                tag_ids: vec![prog.id],
                channels: vec!["UCc".into()],
                ..Default::default()
            },
        )
        .await;
        assert_eq!(tag_or_channel, ["c1", "a2", "a1"]);
        let excluded = ids(
            &db,
            u,
            feed::FeedQuery {
                exclude_channels: vec!["UCb".into()],
                ..Default::default()
            },
        )
        .await;
        assert_eq!(excluded, ["c1", "a2", "a1"]);
        // Another user's tag ids don't match anything.
        let other = ids(
            &db,
            u + 1,
            feed::FeedQuery {
                tag_ids: vec![prog.id],
                ..Default::default()
            },
        )
        .await;
        assert!(other.is_empty());
    }

    #[tokio::test]
    async fn duration_type_date_and_watched() {
        let (db, u) = setup().await;
        let long = ids(
            &db,
            u,
            feed::FeedQuery {
                min_duration_s: Some(600),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(long, ["c1", "b2", "a2"], "unknown duration (b2) is kept");
        let short = ids(
            &db,
            u,
            feed::FeedQuery {
                max_duration_s: Some(100),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(short, ["b2", "b1", "a1"]);
        let q = feed::FeedQuery {
            include_shorts: false,
            include_live: false,
            limit: 50,
            ..Default::default()
        };
        let rows: Vec<_> = feed::feed(db.pool(), u, &q)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(rows, ["c1", "a2", "a1"]);
        let recent = ids(
            &db,
            u,
            feed::FeedQuery {
                published_after: Some(300),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(recent, ["c1", "b2", "b1"]);

        progress::save(db.pool(), u, "a1", 59.0, 60.0)
            .await
            .unwrap(); // finished
        progress::save(db.pool(), u, "a2", 300.0, 1200.0)
            .await
            .unwrap(); // in progress
        let w = |watched| feed::FeedQuery {
            watched,
            ..Default::default()
        };
        assert_eq!(ids(&db, u, w(feed::Watched::InProgress)).await, ["a2"]);
        assert_eq!(
            ids(&db, u, w(feed::Watched::OnlyUnwatched)).await,
            ["c1", "b2", "b1"]
        );
        assert_eq!(
            ids(&db, u, w(feed::Watched::HideFinished)).await,
            ["c1", "b2", "b1", "a2"]
        );
    }

    #[tokio::test]
    async fn hidden_videos_are_left_out() {
        let (db, u) = setup().await;
        hidden::hide(db.pool(), u, "c1").await.unwrap();
        hidden::hide(db.pool(), u, "c1").await.unwrap();
        assert_eq!(
            ids(&db, u, Default::default()).await,
            ["b2", "b1", "a2", "a1"]
        );
        assert!(hidden::unhide(db.pool(), u, "c1").await.unwrap());
        assert_eq!(ids(&db, u, Default::default()).await.len(), 5);
    }

    #[tokio::test]
    async fn tag_management() {
        let (db, u) = setup().await;
        let t = tags::create(db.pool(), u, "Fun", "").await.unwrap();
        assert!(
            tags::create(db.pool(), u, "fun", "").await.is_err(),
            "case-insensitive unique"
        );
        let mut conn = db.acquire().await.unwrap();
        tags::tag_channels(&mut conn, u, t.id, &["UCa".into(), "UCb".into()], true)
            .await
            .unwrap();
        tags::tag_channels(&mut conn, u, t.id, &["UCa".into()], false)
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            tags::channel_tag_pairs(db.pool(), u).await.unwrap(),
            vec![("UCb".to_string(), t.id)]
        );
        assert_eq!(
            tags::count_owned(db.pool(), u, &[t.id, 999]).await.unwrap(),
            1
        );
        assert!(
            tags::update(db.pool(), u, t.id, "Entertainment", "red")
                .await
                .unwrap()
        );
        assert!(!tags::update(db.pool(), u + 1, t.id, "x", "").await.unwrap());
        assert!(tags::delete(db.pool(), u, t.id).await.unwrap());
        assert!(
            tags::channel_tag_pairs(db.pool(), u)
                .await
                .unwrap()
                .is_empty(),
            "cascades"
        );
    }
}

mod playlists {
    use katsanto_db::{
        Db, playlists,
        users::{self, NewUser},
        videos::{self, ChannelMeta, VideoMeta},
    };

    async fn setup() -> (Db, i64) {
        let db = Db::in_memory().await.unwrap();
        let u = users::create(
            db.pool(),
            NewUser {
                username: "p",
                webauthn_id: b"p",
                password_hash: None,
                is_admin: false,
            },
        )
        .await
        .unwrap()
        .id;
        videos::upsert_channel(
            db.pool(),
            &ChannelMeta {
                id: "UC1",
                name: "Chan",
                handle: None,
                avatar_url: None,
                subscriber_count: None,
            },
        )
        .await
        .unwrap();
        for (id, dur) in [("a", 60), ("b", 120), ("c", 180), ("d", 240)] {
            videos::upsert_video(
                db.pool(),
                &VideoMeta {
                    id,
                    channel_id: "UC1",
                    title: id,
                    thumbnail_url: Some(&format!("{id}.jpg")),
                    duration_s: Some(dur),
                    published_at: None,
                    is_short: false,
                    is_live: false,
                },
            )
            .await
            .unwrap();
        }
        (db, u)
    }

    async fn order(db: &Db, u: i64, list: i64) -> Vec<String> {
        playlists::items(db.pool(), u, list)
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.video_id)
            .collect()
    }

    #[tokio::test]
    async fn watch_later_is_created_once() {
        let (db, u) = setup().await;
        let mut conn = db.acquire().await.unwrap();
        let a = playlists::ensure_watch_later(&mut conn, u).await.unwrap();
        let b = playlists::ensure_watch_later(&mut conn, u).await.unwrap();
        assert_eq!(a, b);
        drop(conn);
        assert!(
            !playlists::delete(db.pool(), u, a).await.unwrap(),
            "can't delete Watch later"
        );
        let lists = playlists::list(db.pool(), u).await.unwrap();
        assert_eq!(lists.len(), 1);
        assert!(lists[0].remove_watched);
    }

    #[tokio::test]
    async fn add_move_remove() {
        let (db, u) = setup().await;
        let list = playlists::create(db.pool(), u, "Mine", "").await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        for id in ["a", "b", "c", "d"] {
            assert!(playlists::add(&mut conn, list, id).await.unwrap());
        }
        assert!(
            !playlists::add(&mut conn, list, "a").await.unwrap(),
            "no duplicates"
        );
        assert!(
            playlists::move_item(&mut conn, list, "d", None)
                .await
                .unwrap()
        );
        assert!(
            playlists::move_item(&mut conn, list, "a", Some("c"))
                .await
                .unwrap()
        );
        assert!(
            !playlists::move_item(&mut conn, list, "zz", None)
                .await
                .unwrap()
        );
        assert!(
            !playlists::move_item(&mut conn, list, "a", Some("zz"))
                .await
                .unwrap()
        );
        drop(conn);
        assert_eq!(order(&db, u, list).await, ["d", "b", "c", "a"]);

        let info = playlists::get(db.pool(), u, list).await.unwrap().unwrap();
        assert_eq!(info.item_count, 4);
        assert_eq!(info.total_duration_s, 600);
        assert_eq!(info.thumbnail_url.as_deref(), Some("d.jpg"));
        assert_eq!(
            playlists::containing(db.pool(), u, "b").await.unwrap(),
            vec![list]
        );
        assert!(playlists::remove(db.pool(), list, "b").await.unwrap());
        assert_eq!(order(&db, u, list).await, ["d", "c", "a"]);
        assert!(
            playlists::get(db.pool(), u + 1, list)
                .await
                .unwrap()
                .is_none(),
            "private"
        );
    }

    #[tokio::test]
    async fn repeated_moves_renumber_when_needed() {
        let (db, u) = setup().await;
        let list = playlists::create(db.pool(), u, "Mine", "").await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        for id in ["a", "b", "c"] {
            playlists::add(&mut conn, list, id).await.unwrap();
        }
        // Keep squeezing items between "a" and its neighbour until floats run out.
        for i in 0..120 {
            let id = if i % 2 == 0 { "c" } else { "b" };
            assert!(
                playlists::move_item(&mut conn, list, id, Some("a"))
                    .await
                    .unwrap()
            );
        }
        drop(conn);
        assert_eq!(order(&db, u, list).await, ["a", "b", "c"]);
    }

    #[tokio::test]
    async fn watched_videos_leave_lists_that_ask_for_it() {
        let (db, u) = setup().await;
        let mut conn = db.acquire().await.unwrap();
        let wl = playlists::ensure_watch_later(&mut conn, u).await.unwrap();
        drop(conn);
        let keep = playlists::create(db.pool(), u, "Keep", "").await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        playlists::add(&mut conn, wl, "a").await.unwrap();
        playlists::add(&mut conn, keep, "a").await.unwrap();
        drop(conn);
        assert_eq!(
            playlists::remove_watched(db.pool(), u, "a").await.unwrap(),
            1
        );
        assert!(order(&db, u, wl).await.is_empty());
        assert_eq!(order(&db, u, keep).await, ["a"]);
    }
}

mod backup {
    use katsanto_db::{
        Db, backup, feeds, hidden, playlists, progress, subscriptions, tags,
        users::{self, NewUser},
        videos::{self, ChannelMeta, VideoMeta},
    };

    const CHAN: &str = "UCaaaaaaaaaaaaaaaaaaaaaa";
    const V1: &str = "aaaaaaaaaa1";
    const V2: &str = "aaaaaaaaaa2";

    async fn user(db: &Db, name: &str) -> i64 {
        users::create(
            db.pool(),
            NewUser {
                username: name,
                webauthn_id: name.as_bytes(),
                password_hash: None,
                is_admin: false,
            },
        )
        .await
        .unwrap()
        .id
    }

    /// Alice: a subscription, a tag, a feed using it, two lists, progress, a hidden video.
    async fn alice(db: &Db) -> i64 {
        let u = user(db, "alice").await;
        let p = db.pool();
        videos::upsert_channel(
            p,
            &ChannelMeta {
                id: CHAN,
                name: "Chan",
                handle: Some("@chan"),
                avatar_url: None,
                subscriber_count: None,
            },
        )
        .await
        .unwrap();
        for id in [V1, V2] {
            videos::upsert_video(
                p,
                &VideoMeta {
                    id,
                    channel_id: CHAN,
                    title: id,
                    thumbnail_url: None,
                    duration_s: Some(600),
                    published_at: Some(1),
                    is_short: false,
                    is_live: false,
                },
            )
            .await
            .unwrap();
        }
        subscriptions::subscribe(p, u, CHAN).await.unwrap();
        let tag = tags::create(p, u, "Tech", "blue").await.unwrap();
        feeds::create(
            p,
            u,
            "Tech feed",
            "💻",
            &format!(r#"{{"tag_ids":[{}]}}"#, tag.id),
        )
        .await
        .unwrap();
        let mine = playlists::create(p, u, "Mine", "desc").await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        tags::set_channel_tags(&mut conn, u, CHAN, &[tag.id])
            .await
            .unwrap();
        let wl = playlists::ensure_watch_later(&mut conn, u).await.unwrap();
        playlists::add(&mut conn, wl, V1).await.unwrap();
        playlists::add(&mut conn, mine, V2).await.unwrap();
        playlists::add(&mut conn, mine, V1).await.unwrap();
        drop(conn);
        progress::save(p, u, V1, 120.0, 600.0).await.unwrap();
        hidden::hide(p, u, "zzzzzzzzzzz").await.unwrap();
        users::set_settings(p, u, r#"{"language":"fi"}"#)
            .await
            .unwrap();
        u
    }

    #[tokio::test]
    async fn export_and_restore_into_another_account() {
        let db = Db::in_memory().await.unwrap();
        let a = alice(&db).await;
        let mut conn = db.acquire().await.unwrap();
        let b = backup::export(&mut conn, a).await.unwrap();
        drop(conn);
        assert_eq!((b.videos.len(), b.channels.len(), b.lists.len()), (2, 1, 2));

        // Through JSON, as a real file would be.
        let b: backup::Backup = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        backup::validate(&b).unwrap();

        let bob = user(&db, "bob").await;
        // Bob already has a tag called "tech" (other id) and newer progress on V1.
        let bob_tag = tags::create(db.pool(), bob, "tech", "").await.unwrap();
        let mut conn = db.acquire().await.unwrap();
        let stats = backup::import(&mut conn, bob, &b).await.unwrap();
        assert_eq!(
            stats,
            backup::ImportStats {
                subscriptions: 1,
                tags: 0,
                feeds: 1,
                lists: 1,
                list_items: 3,
                progress: 1,
                hidden: 1
            }
        );
        // A second import adds nothing.
        let again = backup::import(&mut conn, bob, &b).await.unwrap();
        assert_eq!(again, backup::ImportStats::default());
        drop(conn);

        let feed = &feeds::list(db.pool(), bob).await.unwrap()[0];
        assert_eq!(feed.filter, format!(r#"{{"tag_ids":[{}]}}"#, bob_tag.id));
        assert_eq!(
            tags::channel_tag_pairs(db.pool(), bob).await.unwrap(),
            [(CHAN.to_string(), bob_tag.id)]
        );
        let lists = playlists::list(db.pool(), bob).await.unwrap();
        let mine = lists.iter().find(|l| l.name == "Mine").unwrap();
        let order: Vec<_> = playlists::items(db.pool(), bob, mine.id)
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.video_id)
            .collect();
        assert_eq!(order, [V2, V1]);
        let p = progress::get(db.pool(), bob, V1).await.unwrap().unwrap();
        assert_eq!(p.position_s, 120.0);
        let settings = users::by_id(db.pool(), bob)
            .await
            .unwrap()
            .unwrap()
            .settings;
        assert!(settings.contains(r#""language":"fi""#));
    }

    #[tokio::test]
    async fn newer_progress_wins_and_bad_files_are_rejected() {
        let db = Db::in_memory().await.unwrap();
        let a = alice(&db).await;
        let mut conn = db.acquire().await.unwrap();
        let mut b = backup::export(&mut conn, a).await.unwrap();
        drop(conn);
        // Alice watches further after exporting; restoring the old file keeps that.
        progress::save(db.pool(), a, V1, 300.0, 600.0)
            .await
            .unwrap();
        b.progress[0].updated_at -= 10;
        let mut conn = db.acquire().await.unwrap();
        backup::import(&mut conn, a, &b).await.unwrap();
        drop(conn);
        assert_eq!(
            progress::get(db.pool(), a, V1)
                .await
                .unwrap()
                .unwrap()
                .position_s,
            300.0
        );

        b.version = backup::VERSION + 1;
        assert!(matches!(
            backup::validate(&b),
            Err(backup::InvalidBackup::Version(_))
        ));
        b.version = 1;
        b.videos[0].id = "<script>".into();
        assert!(backup::validate(&b).is_err());
        b.format = "something".into();
        assert!(matches!(
            backup::validate(&b),
            Err(backup::InvalidBackup::Format)
        ));
    }
}
