//! The web build's accounts in the interface (docs/authentication.md §2, §10).
//!
//! Signing in, the account menu, the profile page and the administrator's
//! user list exist in the web build only. Like the desktop-only capabilities
//! ([`crate::geneanet`], [`crate::assistant`]) the other way round, the
//! capability is declared here and injected by the binary that has it: the
//! web entry point provides an [`Accounts`] in the Dioxus context, the desktop
//! provides none. Without it, nothing of this module requests anything or
//! draws anything, so the desktop renders exactly what it rendered before.
//!
//! With it, the application first asks `/auth/session`. A server in `none`
//! mode answers so, and the interface stays the desktop's: no sign-in, no
//! account menu, no banner, and no account route is ever called. A server in
//! `oidc` mode either names the signed-in user, whose CSRF token the client
//! then sends with every write, or nobody, and the pages that need an
//! account offer to sign in instead.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;
use futures_util::StreamExt as _;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::api::account::Me;
use crate::i18n::Language;
use crate::router::Route;

/// What only a browser page can do for the account UI.
///
/// Implemented by `oxidgene-web`. Signing in and out leave the application
/// for the identity provider's pages: whole-page navigations, which the
/// in-app router does not make.
pub trait AccountHost: Send + Sync {
    /// Leaves the application for `url`.
    fn navigate(&self, url: &str);

    /// The query string of the page's address (`?…`, or empty), where a
    /// failed sign-in puts its `auth_error`.
    fn query(&self) -> String;
}

/// The capability handle the web build puts in the Dioxus context.
#[derive(Clone)]
pub struct Accounts(Arc<dyn AccountHost>);

impl Accounts {
    #[must_use]
    pub fn new(host: Arc<dyn AccountHost>) -> Self {
        Self(host)
    }

    pub fn navigate(&self, url: &str) {
        self.0.navigate(url);
    }

    fn query(&self) -> String {
        self.0.query()
    }
}

impl std::fmt::Debug for Accounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Accounts")
    }
}

/// The capability, if this build has one: `None` on the desktop.
pub fn use_accounts() -> Option<Accounts> {
    try_use_context::<Accounts>()
}

/// Who is using the application, as far as accounts go.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountState {
    /// No accounts: the desktop, or the web build in `none` mode.
    Absent,
    /// The web build is still asking the server.
    Loading,
    /// The server signs users in, and nobody is signed in.
    Anonymous,
    /// The signed-in user.
    SignedIn(Box<SignedIn>),
}

/// The signed-in user and their session.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedIn {
    pub me: Me,
    /// The provider's account page, for the password and second factor.
    pub account_url: Option<String>,
}

impl AccountState {
    /// The signed-in user, if any.
    pub fn me(&self) -> Option<&Me> {
        match self {
            Self::SignedIn(signed_in) => Some(&signed_in.me),
            _ => None,
        }
    }

    /// Whether the signed-in user is an instance administrator.
    pub fn is_admin(&self) -> bool {
        self.me().is_some_and(Me::is_admin)
    }
}

/// What a page shows in place of its content when the account is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The page itself.
    Open,
    /// An invitation to sign in: the page reads what only a signed-in user
    /// may (docs/authentication.md §4.8).
    SignIn,
}

/// What `route` shows to the user `state` describes. The tree list and the
/// profile need an account; the application settings are the device's.
/// A tree's pages open to an anonymous visitor: a `public` tree, or one a
/// share link opened, has anonymous Visitors (docs/authorization.md §6.3,
/// §6.4). Any other answers `401`, `refused` says so, and the page gives way
/// to the invitation to sign in.
///
/// A page opens while `/auth/session` is still being asked: nothing waits
/// for the answer, so the `none` mode loads as fast as the desktop.
pub fn gate(state: &AccountState, route: &Route, refused: bool) -> Gate {
    let needs_account = match route {
        Route::AppSettings {} | Route::NotFound { .. } => false,
        Route::Home {} | Route::Invitations {} | Route::Profile {} => true,
        other => other.tree_id().is_some() && refused,
    };
    if needs_account && *state == AccountState::Anonymous {
        Gate::SignIn
    } else {
        Gate::Open
    }
}

/// How many requests of an anonymous visitor the server refused with `401`
/// since the application opened: a page compares it with the count at its
/// mount to know whether it was refused.
#[derive(Clone, Copy)]
pub struct AnonymousRefusals(pub Signal<u64>);

/// A dialog the account UI opens over the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountPrompt {
    /// A request was refused with `401` while signed in.
    SessionEnded,
    /// The sign-in came back with an error.
    SignInFailed(AuthError),
}

/// Why a sign-in failed, as `/auth/callback` reports it in `auth_error`
/// (docs/authentication.md §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    AccessDenied,
    LoginFailed,
    SignupClosed,
    AccountDeactivated,
    AuthUnavailable,
    /// A share link that opens nothing: unknown, revoked or expired.
    ShareLinkInvalid,
}

impl AuthError {
    /// The error a page address carries, if any. An unknown code reads as a
    /// failed sign-in rather than being ignored.
    pub fn from_query(query: &str) -> Option<Self> {
        let code = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix("auth_error="))?;
        Some(match code {
            "access_denied" => Self::AccessDenied,
            "signup_closed" => Self::SignupClosed,
            "account_deactivated" => Self::AccountDeactivated,
            "auth_unavailable" => Self::AuthUnavailable,
            "share_link_invalid" => Self::ShareLinkInvalid,
            _ => Self::LoginFailed,
        })
    }

    pub fn message_key(self) -> &'static str {
        match self {
            Self::AccessDenied => "account.auth_error.access_denied",
            Self::LoginFailed => "account.auth_error.login_failed",
            Self::SignupClosed => "account.auth_error.signup_closed",
            Self::AccountDeactivated => "account.auth_error.account_deactivated",
            Self::AuthUnavailable => "account.auth_error.auth_unavailable",
            Self::ShareLinkInvalid => "account.auth_error.share_link_invalid",
        }
    }
}

/// The avatars drawn this session, as `data:` URLs keyed by user, size and
/// version, so that a navigation — which mounts the navbar again — does not
/// fetch the user's own avatar again.
#[derive(Clone, Copy)]
pub struct AvatarImages(pub Signal<HashMap<(Uuid, u32, String), String>>);

/// The account state, provided by [`use_init_account`].
pub fn use_account() -> Signal<AccountState> {
    use_context()
}

/// The open account dialog, provided by [`use_init_account`].
pub fn use_account_prompt() -> Signal<Option<AccountPrompt>> {
    use_context()
}

/// Hook: provide the account state (call once in `AppShell`, after the
/// language), and, in the web build, ask the server who is signed in.
pub fn use_init_account() {
    let accounts = use_accounts();
    let api = use_context::<ApiClient>();
    let language = use_context::<Signal<Language>>();
    let state = use_context_provider(|| {
        Signal::new(if accounts.is_some() {
            AccountState::Loading
        } else {
            AccountState::Absent
        })
    });
    let prompt = use_context_provider(|| Signal::new(None::<AccountPrompt>));
    let refusals = use_context_provider(|| AnonymousRefusals(Signal::new(0)));
    use_context_provider(|| AvatarImages(Signal::new(HashMap::new())));
    let notifications = use_context_provider(|| {
        crate::notifications::NotificationsState(Signal::new(Default::default()))
    });
    use_hook(move || {
        if let Some(accounts) = accounts {
            // Read before the router takes over the address, which drops
            // a query its route does not name.
            let failed = AuthError::from_query(&accounts.query());
            let watch = AccountWatch {
                state,
                prompt,
                refusals,
                notifications: notifications.0,
            };
            spawn(follow_account(api, failed, watch, language));
        }
    });
}

/// What [`follow_account`] keeps current.
struct AccountWatch {
    state: Signal<AccountState>,
    prompt: Signal<Option<AccountPrompt>>,
    refusals: AnonymousRefusals,
    /// The bell's, kept current once someone is signed in.
    notifications: Signal<crate::notifications::Notifications>,
}

/// Learns who is signed in, then turns every `401` of a signed-in page into
/// the session-ended dialog, and counts an anonymous visitor's, for as long
/// as the application runs.
async fn follow_account(
    api: ApiClient,
    failed: Option<AuthError>,
    watch: AccountWatch,
    language: Signal<Language>,
) {
    let AccountWatch {
        mut state,
        mut prompt,
        refusals: AnonymousRefusals(mut refusals),
        notifications,
    } = watch;
    let session = match api.auth_session().await {
        Ok(session) if session.signs_in() => session,
        // `none` mode, or no answer: the interface is the desktop's.
        _ => {
            state.set(AccountState::Absent);
            return;
        }
    };
    if let Some(error) = failed {
        prompt.set(Some(AccountPrompt::SignInFailed(error)));
    }
    let Some(user) = session.user.filter(|_| session.authenticated) else {
        let mut unauthorized = api.watch_unauthorized();
        state.set(AccountState::Anonymous);
        while unauthorized.next().await.is_some() {
            refusals += 1;
        }
        return;
    };
    api.set_csrf_token(session.csrf_token.as_deref());
    let mut unauthorized = api.watch_unauthorized();
    let me = api.me().await.unwrap_or_else(|_| Me::from_session(&user));
    apply_account_language(me.locale.as_deref(), language).await;
    state.set(AccountState::SignedIn(Box::new(SignedIn {
        me,
        account_url: session.account_url,
    })));
    // The bell's notifications, by long poll (docs/messaging.md §6).
    spawn(crate::notifications::follow(api.clone(), notifications));
    while unauthorized.next().await.is_some() {
        if prompt.peek().is_none() {
            prompt.set(Some(AccountPrompt::SessionEnded));
        }
    }
}

/// The account's language becomes the interface's on a device where none
/// was chosen (docs/authentication.md §10). It is not stored as the
/// device's choice, which stays the device's to make.
async fn apply_account_language(locale: Option<&str>, mut language: Signal<Language>) {
    let Some(account_language) = locale.and_then(Language::try_from_code) else {
        return;
    };
    let stored = document::eval("return localStorage.getItem('oxidgene-lang');").await;
    if matches!(stored, Ok(serde_json::Value::Null)) {
        language.set(account_language);
    }
}

/// Replaces the signed-in user's account with `me`, as a change to the
/// profile returned it, so the navbar shows it at once.
pub fn set_me(mut state: Signal<AccountState>, me: Me) {
    if let AccountState::SignedIn(signed_in) = &mut *state.write() {
        signed_in.me = me;
    }
}

/// Sends the browser to the sign-in, which comes back to `route`.
pub fn sign_in(accounts: &Accounts, api: &ApiClient, route: &Route, language: Language) {
    accounts.navigate(&api.sign_in_url(&route.to_string(), language.code()));
}

/// Signs out — of this session, or of every session of the user — and
/// leaves for the provider's sign-out, or the home page when it has none.
/// Either way the page loads afresh, with nothing of the account left in
/// memory.
pub async fn sign_out(
    accounts: &Accounts,
    api: &ApiClient,
    everywhere: bool,
) -> Result<(), crate::api::ApiError> {
    let answer = api.sign_out(everywhere).await?;
    accounts.navigate(answer.logout_url.as_deref().unwrap_or("/"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::account::SessionUser;

    fn signed_in(role: &str) -> AccountState {
        AccountState::SignedIn(Box::new(SignedIn {
            me: Me::from_session(&SessionUser {
                id: Uuid::nil(),
                display_name: Some("Ada Example".into()),
                email: None,
                email_verified: false,
                locale: None,
                instance_role: role.into(),
                can_create_trees: false,
            }),
            account_url: None,
        }))
    }

    #[test]
    fn without_accounts_every_page_opens_at_once() {
        for route in [
            Route::Home {},
            Route::Profile {},
            Route::AppSettings {},
            Route::Statistics {
                tree_id: "t".into(),
            },
        ] {
            assert_eq!(
                gate(&AccountState::Absent, &route, false),
                Gate::Open,
                "{route}"
            );
            assert_eq!(
                gate(&signed_in("user"), &route, true),
                Gate::Open,
                "{route}"
            );
        }
    }

    #[test]
    fn a_visitor_is_asked_to_sign_in_where_an_account_is_needed() {
        let tree = Route::TreeDetail {
            tree_id: "t".into(),
            person: None,
        };
        for route in [Route::Home {}, Route::Profile {}, Route::Invitations {}] {
            assert_eq!(
                gate(&AccountState::Anonymous, &route, false),
                Gate::SignIn,
                "{route}"
            );
            // Nothing waits for the server's answer.
            assert_eq!(
                gate(&AccountState::Loading, &route, false),
                Gate::Open,
                "{route}"
            );
        }
        // A tree opens to an anonymous visitor until the server refuses it.
        assert_eq!(gate(&AccountState::Anonymous, &tree, false), Gate::Open);
        assert_eq!(gate(&AccountState::Anonymous, &tree, true), Gate::SignIn);
        // The application settings are the device's.
        assert_eq!(
            gate(&AccountState::Anonymous, &Route::AppSettings {}, true),
            Gate::Open
        );
    }

    #[test]
    fn only_an_administrator_is_one() {
        assert!(signed_in("admin").is_admin());
        assert!(!signed_in("user").is_admin());
        assert!(!AccountState::Anonymous.is_admin());
    }

    #[test]
    fn a_failed_sign_in_is_read_from_the_address() {
        assert_eq!(
            AuthError::from_query("?auth_error=signup_closed"),
            Some(AuthError::SignupClosed)
        );
        assert_eq!(
            AuthError::from_query("?x=1&auth_error=account_deactivated"),
            Some(AuthError::AccountDeactivated)
        );
        assert_eq!(
            AuthError::from_query("?auth_error=something_new"),
            Some(AuthError::LoginFailed)
        );
        assert_eq!(AuthError::from_query(""), None);
        assert_eq!(AuthError::from_query("?person=p"), None);
    }

    #[test]
    fn every_sign_in_error_is_translated() {
        let english = crate::i18n::Language::english().translations();
        for error in [
            AuthError::AccessDenied,
            AuthError::LoginFailed,
            AuthError::SignupClosed,
            AuthError::AccountDeactivated,
            AuthError::AuthUnavailable,
            AuthError::ShareLinkInvalid,
        ] {
            assert!(english.contains_key(error.message_key()), "{error:?}");
        }
    }
}
