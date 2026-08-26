use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    time::Duration,
};

use serde_json::Value;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

use crate::wire::{
    LEADER_PROTOCOL_VERSION, LeaderClient, LeaderServer, follower_initialize, host_away_note,
    initialize_result, is_interaction, method_of, parse_rpc_text, rpc_error, session_id_in,
    session_load,
};

const REGISTER_WAIT: Duration = Duration::from_secs(10);
const READY_WAIT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub(crate) struct PhoneId(u64);

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub(crate) struct HomeGen(u64);

#[derive(Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub(crate) struct HomeReqId(u64);

#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub(crate) struct SessionId(String);

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub(crate) struct JsonId(Value);

pub(crate) enum Home {
    Away,
    BringingUp(Incoming),
    Sole(Live),
    Dual(Replace),
}

impl Home {
    pub(crate) fn away(&self) -> bool {
        matches!(self, Home::Away | Home::BringingUp(_))
    }
}

pub(crate) struct Live {
    link: Link,
}

pub(crate) struct Link {
    home_gen: HomeGen,
    out: mpsc::UnboundedSender<LeaderClient>,
    halt: Option<oneshot::Sender<()>>,
}

impl Link {
    fn send(&self, msg: LeaderClient) {
        let _ = self.out.send(msg);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(halt) = self.halt.take() {
            let _ = halt.send(());
        }
    }
}

pub(crate) enum Incoming {
    WaitRegistered { link: Link, deadline: Instant },
    WaitReady { link: Link, deadline: Instant },
    WaitInitialize { link: Link },
    WaitLoads { link: Link, pending: PendingLoads },
    Ready { link: Link },
}

impl Incoming {
    fn link(&self) -> &Link {
        match self {
            Incoming::WaitRegistered { link, .. }
            | Incoming::WaitReady { link, .. }
            | Incoming::WaitInitialize { link, .. }
            | Incoming::WaitLoads { link, .. }
            | Incoming::Ready { link } => link,
        }
    }

    fn deadline(&self) -> Option<Instant> {
        match self {
            Incoming::WaitRegistered { deadline, .. } | Incoming::WaitReady { deadline, .. } => {
                Some(*deadline)
            }
            _ => None,
        }
    }

    fn into_live(self) -> Result<Live, Self> {
        match self {
            Incoming::Ready { link } => Ok(Live { link }),
            other => Err(other),
        }
    }

    fn take_link(self) -> Link {
        match self {
            Incoming::WaitRegistered { link, .. }
            | Incoming::WaitReady { link, .. }
            | Incoming::WaitInitialize { link, .. }
            | Incoming::WaitLoads { link, .. }
            | Incoming::Ready { link } => link,
        }
    }
}

pub(crate) struct PendingLoads {
    remaining: BTreeSet<SessionId>,
    load_ids: BTreeMap<HomeReqId, SessionId>,
}

pub(crate) enum LoadProgress {
    More(PendingLoads),
    Done,
    Ignore(PendingLoads),
}

impl PendingLoads {
    pub(crate) fn start(
        watched: impl IntoIterator<Item = SessionId>,
        mut alloc: impl FnMut() -> HomeReqId,
    ) -> Option<Self> {
        let remaining: BTreeSet<_> = watched.into_iter().collect();
        if remaining.is_empty() {
            return None;
        }
        let load_ids = remaining
            .iter()
            .cloned()
            .map(|session| (alloc(), session))
            .collect();
        Some(Self {
            remaining,
            load_ids,
        })
    }

    pub(crate) fn loaded(mut self, id: HomeReqId) -> LoadProgress {
        let Some(session) = self.load_ids.remove(&id) else {
            return LoadProgress::Ignore(self);
        };
        self.remaining.remove(&session);
        if self.remaining.is_empty() {
            LoadProgress::Done
        } else {
            LoadProgress::More(self)
        }
    }

    fn requests(&self) -> impl Iterator<Item = (HomeReqId, &SessionId)> {
        self.load_ids.iter().map(|(id, session)| (*id, session))
    }
}

pub(crate) struct Replace {
    incumbent: Live,
    incoming: Incoming,
}

impl Replace {
    fn abort(self) -> Live {
        let Replace {
            incumbent,
            incoming,
        } = self;
        drop(incoming);
        incumbent
    }

    fn arm(self) -> Result<Armed, Self> {
        let Replace {
            incumbent,
            incoming,
        } = self;
        match incoming.into_live() {
            Ok(successor) => Ok(Armed {
                incumbent,
                successor,
            }),
            Err(incoming) => Err(Replace {
                incumbent,
                incoming,
            }),
        }
    }
}

pub(crate) struct Armed {
    incumbent: Live,
    successor: Live,
}

impl Armed {
    fn cutover(self) -> Live {
        drop(self.incumbent);
        self.successor
    }
}

pub(crate) enum Phone {
    Connected { out: mpsc::UnboundedSender<String> },
    Ready { out: mpsc::UnboundedSender<String> },
}

impl Phone {
    fn out(&self) -> &mpsc::UnboundedSender<String> {
        match self {
            Phone::Connected { out } | Phone::Ready { out } => out,
        }
    }
}

pub(crate) enum Wait {
    Forwarded { phone: PhoneId, origin: JsonId },
    HubInitialize,
    HubLoad,
}

pub(crate) enum Claim {
    Open { home_id: JsonId },
    Taken,
}

impl Claim {
    fn take(&mut self) -> Option<JsonId> {
        match std::mem::replace(self, Claim::Taken) {
            Claim::Open { home_id } => Some(home_id),
            Claim::Taken => None,
        }
    }
}

pub(crate) enum ToHub {
    Accepted {
        out: mpsc::UnboundedSender<LeaderClient>,
        halt: oneshot::Sender<()>,
        bind: oneshot::Sender<HomeGen>,
    },
    HomeFrame {
        home_gen: HomeGen,
        msg: LeaderServer,
    },
    HomeEof {
        home_gen: HomeGen,
    },
    PhoneHello {
        out: mpsc::UnboundedSender<String>,
        bind: oneshot::Sender<PhoneId>,
    },
    PhoneText {
        phone: PhoneId,
        text: String,
    },
    PhoneEof {
        phone: PhoneId,
    },
    Shutdown,
}

pub(crate) struct Hub {
    home: Home,
    phones: HashMap<PhoneId, Phone>,
    watchers: HashMap<SessionId, HashSet<PhoneId>>,
    driver: HashMap<SessionId, PhoneId>,
    outstanding: HashMap<HomeReqId, Wait>,
    reverse: HashMap<(PhoneId, JsonId), HomeReqId>,
    claims: HashMap<HomeReqId, Claim>,
    next_phone: PhoneId,
    next_req: HomeReqId,
    next_gen: HomeGen,
    reverse_seq: u64,
    refused_version: Option<u32>,
}

impl Hub {
    pub(crate) fn new() -> Self {
        Self {
            home: Home::Away,
            phones: HashMap::new(),
            watchers: HashMap::new(),
            driver: HashMap::new(),
            outstanding: HashMap::new(),
            reverse: HashMap::new(),
            claims: HashMap::new(),
            next_phone: PhoneId(0),
            next_req: HomeReqId(0),
            next_gen: HomeGen(0),
            reverse_seq: 0,
            refused_version: None,
        }
    }

    pub(crate) async fn run(mut self, mut rx: mpsc::UnboundedReceiver<ToHub>) {
        loop {
            let deadline = self.current_deadline();
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else {
                        self.shutdown();
                        break;
                    };
                    if matches!(msg, ToHub::Shutdown) {
                        self.shutdown();
                        break;
                    }
                    self.handle(msg);
                }
                _ = sleep_until(deadline) => {
                    self.on_deadline();
                }
            }
        }
    }

    fn handle(&mut self, msg: ToHub) {
        match msg {
            ToHub::Accepted { out, halt, bind } => self.on_accepted(out, halt, bind),
            ToHub::HomeFrame { home_gen, msg } => self.on_home_frame(home_gen, msg),
            ToHub::HomeEof { home_gen } => self.on_home_eof(home_gen),
            ToHub::PhoneHello { out, bind } => self.on_phone_hello(out, bind),
            ToHub::PhoneText { phone, text } => self.on_phone_text(phone, text),
            ToHub::PhoneEof { phone } => self.on_phone_eof(phone),
            ToHub::Shutdown => self.shutdown(),
        }
    }

    fn current_deadline(&self) -> Option<Instant> {
        match &self.home {
            Home::BringingUp(incoming) | Home::Dual(Replace { incoming, .. }) => {
                incoming.deadline()
            }
            _ => None,
        }
    }

    fn on_accepted(
        &mut self,
        out: mpsc::UnboundedSender<LeaderClient>,
        halt: oneshot::Sender<()>,
        bind: oneshot::Sender<HomeGen>,
    ) {
        self.next_gen.0 += 1;
        let home_gen = self.next_gen;
        let _ = bind.send(home_gen);
        let link = Link {
            home_gen,
            out,
            halt: Some(halt),
        };
        let incoming = Incoming::WaitRegistered {
            link,
            deadline: Instant::now() + REGISTER_WAIT,
        };
        match std::mem::replace(&mut self.home, Home::Away) {
            Home::Away => {
                self.home = Home::BringingUp(incoming);
            }
            Home::Sole(incumbent) => {
                self.home = Home::Dual(Replace {
                    incumbent,
                    incoming,
                });
            }
            Home::Dual(rep) => {
                let incumbent = rep.abort();
                self.home = Home::Dual(Replace {
                    incumbent,
                    incoming,
                });
            }
            Home::BringingUp(old) => {
                drop(old);
                self.home = Home::BringingUp(incoming);
            }
        }
    }

    fn on_home_frame(&mut self, home_gen: HomeGen, msg: LeaderServer) {
        match self.role(home_gen) {
            Role::Unknown => {}
            Role::Mux | Role::Candidate => match msg {
                LeaderServer::Registered {
                    ready,
                    leader_protocol_version,
                } => {
                    if self.role(home_gen) == Role::Candidate {
                        self.on_registered(home_gen, ready, leader_protocol_version);
                    }
                }
                LeaderServer::LeaderReady => {
                    if self.role(home_gen) == Role::Candidate {
                        self.on_leader_ready(home_gen);
                    }
                }
                LeaderServer::Acp { payload } => self.on_home_acp(home_gen, payload),
                LeaderServer::Pong | LeaderServer::Unknown => {}
                LeaderServer::Error { message } => {
                    if self.role(home_gen) == Role::Candidate {
                        eprintln!("topside: leader error during handshake: {message}");
                        self.fail_incoming(home_gen);
                    } else {
                        self.on_home_eof(home_gen);
                    }
                }
            },
        }
    }

    fn on_registered(&mut self, home_gen: HomeGen, ready: bool, version: u32) {
        if version != LEADER_PROTOCOL_VERSION {
            eprintln!(
                "topside: refused leader_protocol_version={version} (want {LEADER_PROTOCOL_VERSION})"
            );
            self.refused_version = Some(version);
            self.fail_incoming(home_gen);
            return;
        }
        self.refused_version = None;
        let Some((incoming, slot)) = self.take_candidate(home_gen) else {
            return;
        };
        if !matches!(incoming, Incoming::WaitRegistered { .. }) {
            self.put_candidate(incoming, slot);
            return;
        }
        let link = incoming.take_link();
        if ready {
            self.send_follower_initialize(&link);
            self.put_candidate(Incoming::WaitInitialize { link }, slot);
        } else {
            self.put_candidate(
                Incoming::WaitReady {
                    link,
                    deadline: Instant::now() + READY_WAIT,
                },
                slot,
            );
        }
    }

    fn on_leader_ready(&mut self, home_gen: HomeGen) {
        let Some((incoming, slot)) = self.take_candidate(home_gen) else {
            return;
        };
        if !matches!(incoming, Incoming::WaitReady { .. }) {
            self.put_candidate(incoming, slot);
            return;
        }
        let link = incoming.take_link();
        self.send_follower_initialize(&link);
        self.put_candidate(Incoming::WaitInitialize { link }, slot);
    }

    fn send_follower_initialize(&mut self, link: &Link) {
        let id = self.alloc_req();
        self.outstanding.insert(id, Wait::HubInitialize);
        link.send(LeaderClient::acp(follower_initialize(id.0)));
    }

    fn on_home_acp(&mut self, home_gen: HomeGen, payload: String) {
        let Ok(obj) = parse_rpc_text(&payload) else {
            return;
        };
        let has_method = obj.get("method").and_then(Value::as_str).is_some();
        let has_id = obj.get("id").is_some();
        if has_method && has_id {
            if self.role(home_gen) == Role::Mux {
                self.on_reverse_request(obj);
            }
            return;
        }
        if has_method {
            if self.role(home_gen) == Role::Mux {
                self.on_home_notification(obj);
            }
            return;
        }
        if has_id {
            self.on_home_result(home_gen, obj);
        }
    }

    fn on_home_result(&mut self, home_gen: HomeGen, obj: Value) {
        let Some(id) = obj.get("id").and_then(Value::as_u64).map(HomeReqId) else {
            return;
        };
        let Some(wait) = self.outstanding.remove(&id) else {
            return;
        };
        match wait {
            Wait::Forwarded { phone, origin } => {
                if self.role(home_gen) != Role::Mux {
                    return;
                }
                if let Some(session) = session_id_in(&obj) {
                    let _ = self.watch(phone, SessionId(session));
                }
                let mut reply = obj;
                reply["id"] = origin.0;
                self.send_phone(phone, reply.to_string());
            }
            Wait::HubInitialize => {
                if self.role(home_gen) != Role::Candidate {
                    return;
                }
                if obj.get("error").is_some() {
                    self.fail_incoming(home_gen);
                    return;
                }
                self.after_follower_initialize(home_gen);
            }
            Wait::HubLoad => {
                if self.role(home_gen) != Role::Candidate {
                    return;
                }
                self.after_load_result(home_gen, id);
            }
        }
    }

    fn after_follower_initialize(&mut self, home_gen: HomeGen) {
        let Some((incoming, slot)) = self.take_candidate(home_gen) else {
            return;
        };
        if !matches!(incoming, Incoming::WaitInitialize { .. }) {
            self.put_candidate(incoming, slot);
            return;
        }
        let link = incoming.take_link();
        let watched: Vec<SessionId> = self.watchers.keys().cloned().collect();
        let mut next_req = self.next_req;
        let pending = PendingLoads::start(watched, || {
            next_req.0 += 1;
            next_req
        });
        self.next_req = next_req;
        match pending {
            None => self.finish_successor(Live { link }, slot),
            Some(pending) => {
                for (id, session) in pending.requests() {
                    self.outstanding.insert(id, Wait::HubLoad);
                    link.send(LeaderClient::acp(session_load(id.0, &session.0)));
                }
                self.put_candidate(Incoming::WaitLoads { link, pending }, slot);
            }
        }
    }

    fn after_load_result(&mut self, home_gen: HomeGen, id: HomeReqId) {
        let Some((incoming, slot)) = self.take_candidate(home_gen) else {
            return;
        };
        let Incoming::WaitLoads { link, pending } = incoming else {
            self.put_candidate(incoming, slot);
            return;
        };
        match pending.loaded(id) {
            LoadProgress::More(pending) | LoadProgress::Ignore(pending) => {
                self.put_candidate(Incoming::WaitLoads { link, pending }, slot);
            }
            LoadProgress::Done => self.finish_successor(Live { link }, slot),
        }
    }

    fn finish_successor(&mut self, successor: Live, slot: CandidateSlot) {
        match slot.incumbent {
            None => {
                self.home = Home::Sole(successor);
                self.broadcast_away(false);
            }
            Some(incumbent) => {
                let incoming = Incoming::Ready {
                    link: successor.link,
                };
                match (Replace {
                    incumbent,
                    incoming,
                })
                .arm()
                {
                    Ok(armed) => self.home = Home::Sole(armed.cutover()),
                    Err(rep) => self.home = Home::Dual(rep),
                }
            }
        }
    }

    fn on_home_notification(&mut self, obj: Value) {
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return;
        };
        if method_of(method) != "session/update" {
            return;
        }
        let Some(session) = session_id_in(&obj).map(SessionId) else {
            return;
        };
        let Some(watchers) = self.watchers.get(&session) else {
            return;
        };
        let text = obj.to_string();
        for phone in watchers.clone() {
            self.send_phone(phone, text.clone());
        }
    }

    fn on_reverse_request(&mut self, obj: Value) {
        let Some(method) = obj.get("method").and_then(Value::as_str).map(str::to_owned) else {
            return;
        };
        let Some(home_id) = obj.get("id").cloned() else {
            return;
        };
        let Some(session) = session_id_in(&obj).map(SessionId) else {
            return;
        };
        let interaction = is_interaction(&method);
        let targets: Vec<PhoneId> = if interaction {
            self.watchers
                .get(&session)
                .into_iter()
                .flatten()
                .copied()
                .collect()
        } else {
            self.driver.get(&session).copied().into_iter().collect()
        };
        if targets.is_empty() {
            return;
        }
        let claim_key = self.alloc_req();
        self.claims.insert(
            claim_key,
            Claim::Open {
                home_id: JsonId(home_id),
            },
        );
        for phone in targets {
            let phone_id = self.alloc_reverse_id();
            self.reverse.insert((phone, phone_id.clone()), claim_key);
            let mut for_phone = obj.clone();
            for_phone["id"] = phone_id.0;
            self.send_phone(phone, for_phone.to_string());
        }
    }

    fn on_home_eof(&mut self, home_gen: HomeGen) {
        match self.role(home_gen) {
            Role::Unknown => {}
            Role::Mux => match std::mem::replace(&mut self.home, Home::Away) {
                Home::Sole(live) if live.link.home_gen == home_gen => {
                    drop(live);
                    self.home = Home::Away;
                    self.broadcast_away(true);
                }
                Home::Dual(rep) if rep.incumbent.link.home_gen == home_gen => {
                    let Replace {
                        incumbent,
                        incoming,
                    } = rep;
                    drop(incumbent);
                    self.home = Home::BringingUp(incoming);
                    self.broadcast_away(true);
                }
                other => self.home = other,
            },
            Role::Candidate => {
                self.fail_incoming(home_gen);
            }
        }
    }

    fn fail_incoming(&mut self, home_gen: HomeGen) {
        match std::mem::replace(&mut self.home, Home::Away) {
            Home::BringingUp(incoming) if incoming.link().home_gen == home_gen => {
                drop(incoming);
                self.home = Home::Away;
            }
            Home::Dual(rep) if rep.incoming.link().home_gen == home_gen => {
                self.home = Home::Sole(rep.abort());
            }
            other => self.home = other,
        }
    }

    fn on_deadline(&mut self) {
        match std::mem::replace(&mut self.home, Home::Away) {
            Home::BringingUp(incoming) if incoming.deadline().is_some() => {
                drop(incoming);
                self.home = Home::Away;
            }
            Home::Dual(rep) if rep.incoming.deadline().is_some() => {
                self.home = Home::Sole(rep.abort());
            }
            other => self.home = other,
        }
    }

    fn on_phone_hello(
        &mut self,
        out: mpsc::UnboundedSender<String>,
        bind: oneshot::Sender<PhoneId>,
    ) {
        self.next_phone.0 += 1;
        let id = self.next_phone;
        let _ = bind.send(id);
        self.phones.insert(id, Phone::Connected { out });
    }

    fn on_phone_text(&mut self, phone: PhoneId, text: String) {
        if !self.phones.contains_key(&phone) {
            return;
        }
        match parse_rpc_text(&text) {
            Ok(obj) => self.on_rpc(phone, obj),
            Err(_) => {
                self.send_phone(phone, rpc_error(Value::Null, -32700, "Parse error"));
            }
        }
    }

    fn on_rpc(&mut self, phone: PhoneId, obj: Value) {
        let method = obj.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = obj.get("id").cloned();
        if let Some(method) = method {
            if let Some(id) = id {
                self.on_phone_request(phone, &method, id, obj);
            } else {
                self.forward_notification(obj);
            }
            return;
        }
        if (obj.get("result").is_some() || obj.get("error").is_some())
            && let Some(id) = id
        {
            self.on_phone_reply(phone, JsonId(id), obj);
        }
    }

    fn on_phone_request(&mut self, phone: PhoneId, method: &str, id: Value, obj: Value) {
        match method {
            "initialize" => {
                self.answer_initialize(phone, id);
            }
            "authenticate" => {
                self.send_phone(phone, rpc_error(id, -32601, "Method not found"));
            }
            "session/load" | "session/resume" => {
                if !self.phone_ready(phone) {
                    self.send_phone(phone, rpc_error(id, -32600, "phone has not initialized"));
                    return;
                }
                if let Some(session) = session_id_in(&obj) {
                    let _ = self.watch(phone, SessionId(session));
                }
                self.forward_request(phone, id, obj);
            }
            "session/close" => {
                if let Some(session) = session_id_in(&obj) {
                    self.unwatch(phone, &SessionId(session));
                }
                self.forward_request(phone, id, obj);
            }
            _ => {
                if method.starts_with("session/") && !self.phone_ready(phone) {
                    self.send_phone(phone, rpc_error(id, -32600, "phone has not initialized"));
                    return;
                }
                self.forward_request(phone, id, obj);
            }
        }
    }

    fn answer_initialize(&mut self, phone: PhoneId, id: Value) {
        let Some(slot) = self.phones.remove(&phone) else {
            return;
        };
        let out = match slot {
            Phone::Connected { out } | Phone::Ready { out } => out,
        };
        let _ = out.send(initialize_result(&id));
        if self.home.away() {
            let _ = out.send(host_away_note(true));
        }
        self.phones.insert(phone, Phone::Ready { out });
    }

    fn forward_request(&mut self, phone: PhoneId, origin: Value, mut obj: Value) {
        let Some(out) = self.mux_out() else {
            self.send_phone(phone, self.away_error(origin));
            return;
        };
        let home_id = self.alloc_req();
        self.outstanding.insert(
            home_id,
            Wait::Forwarded {
                phone,
                origin: JsonId(origin),
            },
        );
        obj["id"] = Value::from(home_id.0);
        let _ = out.send(LeaderClient::acp(obj.to_string()));
    }

    fn forward_notification(&mut self, obj: Value) {
        let Some(out) = self.mux_out() else {
            return;
        };
        let _ = out.send(LeaderClient::acp(obj.to_string()));
    }

    fn on_phone_reply(&mut self, phone: PhoneId, origin: JsonId, mut obj: Value) {
        let Some(claim_key) = self.reverse.remove(&(phone, origin)) else {
            return;
        };
        let home_id = {
            let Some(claim) = self.claims.get_mut(&claim_key) else {
                return;
            };
            let Some(home_id) = claim.take() else {
                return;
            };
            home_id
        };
        obj["id"] = home_id.0;
        if let Some(out) = self.mux_out() {
            let _ = out.send(LeaderClient::acp(obj.to_string()));
        }
    }

    fn on_phone_eof(&mut self, phone: PhoneId) {
        self.phones.remove(&phone);
        self.watchers.retain(|_, set| {
            set.remove(&phone);
            !set.is_empty()
        });
        let orphaned: Vec<SessionId> = self
            .driver
            .iter()
            .filter_map(|(session, owner)| {
                if *owner == phone {
                    Some(session.clone())
                } else {
                    None
                }
            })
            .collect();
        for session in orphaned {
            if let Some(set) = self.watchers.get(&session) {
                if let Some(&next) = set.iter().next() {
                    self.driver.insert(session, next);
                } else {
                    self.driver.remove(&session);
                }
            } else {
                self.driver.remove(&session);
            }
        }
        self.reverse.retain(|(p, _), _| *p != phone);
    }

    fn watch(&mut self, phone: PhoneId, session: SessionId) -> Result<(), ()> {
        match self.phones.get(&phone) {
            Some(Phone::Ready { .. }) => {
                self.watchers
                    .entry(session.clone())
                    .or_default()
                    .insert(phone);
                self.driver.entry(session).or_insert(phone);
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn unwatch(&mut self, phone: PhoneId, session: &SessionId) {
        if let Some(set) = self.watchers.get_mut(session) {
            set.remove(&phone);
            if set.is_empty() {
                self.watchers.remove(session);
                self.driver.remove(session);
            } else if self.driver.get(session) == Some(&phone) {
                if let Some(&next) = set.iter().next() {
                    self.driver.insert(session.clone(), next);
                }
            }
        }
    }

    fn phone_ready(&self, phone: PhoneId) -> bool {
        matches!(self.phones.get(&phone), Some(Phone::Ready { .. }))
    }

    fn mux_link(&self) -> Option<&Link> {
        match &self.home {
            Home::Sole(live) => Some(&live.link),
            Home::Dual(rep) => Some(&rep.incumbent.link),
            _ => None,
        }
    }

    fn mux_out(&self) -> Option<mpsc::UnboundedSender<LeaderClient>> {
        self.mux_link().map(|link| link.out.clone())
    }

    fn send_phone(&self, phone: PhoneId, text: String) {
        if let Some(slot) = self.phones.get(&phone) {
            let _ = slot.out().send(text);
        }
    }

    fn broadcast_away(&self, away: bool) {
        let note = host_away_note(away);
        for slot in self.phones.values() {
            if let Phone::Ready { out } = slot {
                let _ = out.send(note.clone());
            }
        }
    }

    fn away_error(&self, id: Value) -> String {
        let message = match self.refused_version {
            Some(version) => {
                format!("host is away (unsupported leader protocol version {version})")
            }
            None => "host is away".into(),
        };
        rpc_error(id, -32003, &message)
    }

    fn alloc_req(&mut self) -> HomeReqId {
        self.next_req.0 += 1;
        self.next_req
    }

    fn alloc_reverse_id(&mut self) -> JsonId {
        self.reverse_seq += 1;
        JsonId(Value::String(format!("aq:{}", self.reverse_seq)))
    }

    fn role(&self, home_gen: HomeGen) -> Role {
        match &self.home {
            Home::Sole(live) if live.link.home_gen == home_gen => Role::Mux,
            Home::BringingUp(incoming) if incoming.link().home_gen == home_gen => Role::Candidate,
            Home::Dual(rep) if rep.incumbent.link.home_gen == home_gen => Role::Mux,
            Home::Dual(rep) if rep.incoming.link().home_gen == home_gen => Role::Candidate,
            _ => Role::Unknown,
        }
    }

    fn take_candidate(&mut self, home_gen: HomeGen) -> Option<(Incoming, CandidateSlot)> {
        match std::mem::replace(&mut self.home, Home::Away) {
            Home::BringingUp(incoming) if incoming.link().home_gen == home_gen => {
                Some((incoming, CandidateSlot { incumbent: None }))
            }
            Home::Dual(rep) if rep.incoming.link().home_gen == home_gen => {
                let Replace {
                    incumbent,
                    incoming,
                } = rep;
                Some((
                    incoming,
                    CandidateSlot {
                        incumbent: Some(incumbent),
                    },
                ))
            }
            other => {
                self.home = other;
                None
            }
        }
    }

    fn put_candidate(&mut self, incoming: Incoming, slot: CandidateSlot) {
        match slot.incumbent {
            None => self.home = Home::BringingUp(incoming),
            Some(incumbent) => {
                self.home = Home::Dual(Replace {
                    incumbent,
                    incoming,
                });
            }
        }
    }

    fn shutdown(&mut self) {
        match std::mem::replace(&mut self.home, Home::Away) {
            Home::Away => {}
            Home::BringingUp(incoming) => drop(incoming),
            Home::Sole(live) => drop(live),
            Home::Dual(rep) => {
                drop(rep.abort());
            }
        }
        self.phones.clear();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Mux,
    Candidate,
    Unknown,
}

struct CandidateSlot {
    incumbent: Option<Live>,
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_link(home_gen: u64) -> (Link, oneshot::Receiver<()>) {
        let (out, _) = mpsc::unbounded_channel();
        let (halt, halt_rx) = oneshot::channel();
        (
            Link {
                home_gen: HomeGen(home_gen),
                out,
                halt: Some(halt),
            },
            halt_rx,
        )
    }

    #[test]
    fn pending_loads_empty_skips_reloading() {
        assert!(PendingLoads::start(Vec::<SessionId>::new(), || HomeReqId(1)).is_none());
    }

    #[test]
    fn pending_loads_done_after_last_result() {
        let mut n = 0;
        let pending = PendingLoads::start([SessionId("s1".into())], || {
            n += 1;
            HomeReqId(n)
        })
        .unwrap();
        let id = pending.requests().next().unwrap().0;
        assert!(matches!(pending.loaded(id), LoadProgress::Done));
    }

    #[test]
    fn pending_loads_more_until_all_ids() {
        let mut n = 0;
        let pending = PendingLoads::start([SessionId("a".into()), SessionId("b".into())], || {
            n += 1;
            HomeReqId(n)
        })
        .unwrap();
        let ids: Vec<_> = pending.requests().map(|(id, _)| id).collect();
        let progress = pending.loaded(ids[0]);
        let LoadProgress::More(pending) = progress else {
            panic!("expected more");
        };
        assert!(matches!(pending.loaded(ids[1]), LoadProgress::Done));
    }

    #[test]
    fn arm_rejects_unready_incoming() {
        let (inc, _) = dummy_link(1);
        let (cand, _) = dummy_link(2);
        let rep = Replace {
            incumbent: Live { link: inc },
            incoming: Incoming::WaitRegistered {
                link: cand,
                deadline: Instant::now(),
            },
        };
        match rep.arm() {
            Ok(_) => panic!("unready incoming must not arm"),
            Err(rep) => drop(rep.abort()),
        }
    }

    #[test]
    fn cutover_is_the_only_incumbent_drop() {
        let (inc, inc_halt) = dummy_link(1);
        let (succ, mut succ_halt) = dummy_link(2);
        let rep = Replace {
            incumbent: Live { link: inc },
            incoming: Incoming::Ready { link: succ },
        };
        let armed = rep.arm().ok().expect("ready incoming arms");
        let live = armed.cutover();
        assert_eq!(live.link.home_gen, HomeGen(2));
        assert!(inc_halt.blocking_recv().is_ok());
        assert!(succ_halt.try_recv().is_err());
    }
}
