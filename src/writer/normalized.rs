// Copyright (c) 2018-2021  Brendan Molloy <brendan@bbqsrc.net>,
//                          Ilya Solovyiov <ilya.solovyiov@gmail.com>,
//                          Kai Ren <tyranron@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! [`Writer`]-wrapper for outputting events in a normalized readable order.

use std::sync::Arc;

use async_trait::async_trait;
use either::Either;
use linked_hash_map::LinkedHashMap;

use crate::{event, World, Writer};

/// Wrapper for a [`Writer`] implementation for outputting events in a
/// normalized readable order.
///
/// Doesn't output anything by itself, but rather is used as a combinator for
/// rearranging events and sourcing them to the underlying [`Writer`].
///
/// If some [`Feature`]([`Rule`]/[`Scenario`]/[`Step`]) has started to be
/// written into an output, then it will be written uninterruptedly until its
/// end, even if some other [`Feature`]s have finished their execution. It makes
/// much easier to understand what is really happening in the running
/// [`Feature`].
///
/// [`Feature`]: gherkin::Feature
/// [`Rule`]: gherkin::Rule
/// [`Scenario`]: gherkin::Scenario
/// [`Step`]: gherkin::Step
#[derive(Debug)]
pub struct Normalized<World, Writer> {
    /// [`Writer`] to normalize output of.
    writer: Writer,

    /// Normalization queue of happened events.
    queue: CucumberQueue<World>,
}

impl<W: World, Writer> Normalized<W, Writer> {
    /// Creates a new [`Normalized`] wrapper, which will rearrange events and
    /// feed them to the given [`Writer`].
    pub fn new(writer: Writer) -> Self {
        Self {
            writer,
            queue: CucumberQueue::new(),
        }
    }
}

#[async_trait(?Send)]
impl<World, Wr: Writer<World>> Writer<World> for Normalized<World, Wr> {
    async fn handle_event(&mut self, ev: event::Cucumber<World>) {
        use event::{Cucumber, Feature, Rule};

        match ev {
            Cucumber::Started => {
                self.writer.handle_event(Cucumber::Started).await;
            }
            Cucumber::ParsingError(err) => {
                self.writer.handle_event(Cucumber::ParsingError(err)).await;
            }
            Cucumber::Finished => self.queue.finished(),
            Cucumber::Feature(f, ev) => match ev {
                Feature::Started => self.queue.new_feature(f),
                Feature::Scenario(s, ev) => {
                    self.queue.insert_scenario_event(&f, None, s, ev);
                }
                Feature::Finished => self.queue.feature_finished(&f),
                Feature::Rule(r, ev) => match ev {
                    Rule::Started => self.queue.new_rule(&f, r),
                    Rule::Scenario(s, ev) => {
                        self.queue.insert_scenario_event(&f, Some(r), s, ev);
                    }
                    Rule::Finished => self.queue.rule_finished(&f, r),
                },
            },
        }

        while let Some(feature_to_remove) =
            self.queue.emit_feature_events(&mut self.writer).await
        {
            self.queue.remove(&feature_to_remove);
        }

        if self.queue.is_finished() {
            self.writer.handle_event(Cucumber::Finished).await;
        }
    }
}

/// Normalization queue for all incoming events.
///
/// We use [`LinkedHashMap`] everywhere throughout this module to ensure FIFO
/// queue for our events. This means by calling [`next()`] we reliably get
/// the currently outputting [`Feature`]. We're doing that until it yields
/// [`Feature::Finished`] event after which we remove the current [`Feature`],
/// as all its events have been printed out and we should do it all over again
/// with a [`next()`] [`Feature`].
///
/// [`next()`]: std::iter::Iterator::next()
/// [`Feature`]: gherkin::Feature
/// [`Feature::Finished`]: event::Feature::Finished
#[derive(Debug)]
struct CucumberQueue<World> {
    /// Collected incoming events.
    events: LinkedHashMap<Arc<gherkin::Feature>, FeatureQueue<World>>,

    /// Indicator whether this queue has been finished.
    finished: bool,
}

impl<World> CucumberQueue<World> {
    /// Creates a new [`CucumberQueue`].
    fn new() -> Self {
        Self {
            events: LinkedHashMap::new(),
            finished: false,
        }
    }

    /// Marks this [`CucumberQueue`] as finished on
    /// [`event::Cucumber::Finished`].
    fn finished(&mut self) {
        self.finished = true;
    }

    /// Checks whether [`event::Cucumber::Finished`] has been received.
    fn is_finished(&self) -> bool {
        self.finished
    }

    /// Inserts a new [`Feature`] on [`event::Feature::Started`].
    ///
    /// [`Feature`]: gherkin::Feature
    fn new_feature(&mut self, feature: Arc<gherkin::Feature>) {
        drop(self.events.insert(feature, FeatureQueue::new()));
    }

    /// Marks a [`Feature`] as finished on [`event::Feature::Finished`].
    ///
    /// We don't emit it by the way, as there may be other in-progress
    /// [`Feature`]s which hold the output.
    ///
    /// [`Feature`]: gherkin::Feature
    fn feature_finished(&mut self, feature: &gherkin::Feature) {
        self.events
            .get_mut(feature)
            .unwrap_or_else(|| panic!("No Feature {}", feature.name))
            .finished = true;
    }

    /// Inserts a new [`Rule`] on [`event::Rule::Started`].
    ///
    /// [`Rule`]: gherkin::Feature
    fn new_rule(
        &mut self,
        feature: &gherkin::Feature,
        rule: Arc<gherkin::Rule>,
    ) {
        self.events
            .get_mut(feature)
            .unwrap_or_else(|| panic!("No Feature {}", feature.name))
            .events
            .new_rule(rule);
    }

    /// Marks [`Rule`] as finished on [`event::Rule::Finished`].
    ///
    /// We don't emit it by the way, as there may be other in-progress [`Rule`]s
    /// which hold the output.
    ///
    /// [`Rule`]: gherkin::Feature
    fn rule_finished(
        &mut self,
        feature: &gherkin::Feature,
        rule: Arc<gherkin::Rule>,
    ) {
        self.events
            .get_mut(feature)
            .unwrap_or_else(|| panic!("No Feature {}", feature.name))
            .events
            .rule_finished(rule);
    }

    /// Inserts a new [`event::Scenario::Started`].
    fn insert_scenario_event(
        &mut self,
        feature: &gherkin::Feature,
        rule: Option<Arc<gherkin::Rule>>,
        scenario: Arc<gherkin::Scenario>,
        event: event::Scenario<World>,
    ) {
        self.events
            .get_mut(feature)
            .unwrap_or_else(|| panic!("No Feature {}", feature.name))
            .events
            .insert_scenario_event(rule, scenario, event);
    }

    /// Returns currently outputted [`Feature`].
    ///
    /// [`Feature`]: gherkin::Feature
    fn next_feature(
        &mut self,
    ) -> Option<(Arc<gherkin::Feature>, &mut FeatureQueue<World>)> {
        self.events.iter_mut().next().map(|(f, ev)| (f.clone(), ev))
    }

    /// Removes the given [`Feature`]. Should be called once [`Feature`] was
    /// fully outputted.
    ///
    /// [`Feature`]: gherkin::Feature
    fn remove(&mut self, feature: &gherkin::Feature) {
        drop(self.events.remove(feature));
    }

    /// Emits all ready [`Feature`] events. If some [`Feature`] was fully
    /// outputted, returns it. After that it should be [`remove`]d.
    ///
    /// [`Feature`]: gherkin::Feature
    /// [`remove`]: CucumberQueue::remove()
    async fn emit_feature_events<Wr: Writer<World>>(
        &mut self,
        writer: &mut Wr,
    ) -> Option<Arc<gherkin::Feature>> {
        if let Some((f, events)) = self.next_feature() {
            if !events.is_started() {
                writer
                    .handle_event(event::Cucumber::feature_started(f.clone()))
                    .await;
                events.started();
            }

            while let Some(scenario_or_rule_to_remove) = events
                .emit_scenario_and_rule_events(f.clone(), writer)
                .await
            {
                events.remove(&scenario_or_rule_to_remove);
            }

            if events.is_finished() {
                writer
                    .handle_event(event::Cucumber::feature_finished(f.clone()))
                    .await;
                return Some(f.clone());
            }
        }
        None
    }
}

/// Queue of all events of a single [`Feature`].
///
/// [`Feature`]: gherkin::Feature
#[derive(Debug)]
struct FeatureQueue<World> {
    /// Indicator whether this queue has been started to be written into output.
    started_emitted: bool,

    /// Collected events of this queue.
    events: RulesAndScenariosQueue<World>,

    /// Indicator whether this queue has been finished.
    finished: bool,
}

impl<World> FeatureQueue<World> {
    /// Creates a new [`FeatureQueue`].
    fn new() -> Self {
        Self {
            started_emitted: false,
            events: RulesAndScenariosQueue::new(),
            finished: false,
        }
    }

    /// Checks if [`event::Feature::Started`] has been emitted.
    fn is_started(&self) -> bool {
        self.started_emitted
    }

    /// Marks that [`event::Feature::Started`] was emitted.
    fn started(&mut self) {
        self.started_emitted = true;
    }

    /// Checks if [`event::Feature::Finished`] has been emitted.
    fn is_finished(&self) -> bool {
        self.finished
    }

    /// Removes the given [`RuleOrScenario`].
    ///
    /// Should be called once this [`RuleOrScenario`] was fully outputted.
    fn remove(&mut self, rule_or_scenario: &RuleOrScenario) {
        drop(self.events.0.remove(rule_or_scenario));
    }

    /// Emits all ready [`RuleOrScenario`] events. If some [`RuleOrScenario`]
    /// was fully outputted, then returns it. After that it should be
    /// [`remove`]d.
    ///
    /// [`remove`]: FeatureQueue::remove()
    async fn emit_scenario_and_rule_events<Wr: Writer<World>>(
        &mut self,
        feature: Arc<gherkin::Feature>,
        writer: &mut Wr,
    ) -> Option<RuleOrScenario> {
        match self.events.next_rule_or_scenario()? {
            Either::Left((rule, events)) => events
                .emit_rule_events(feature, rule, writer)
                .await
                .map(Either::Left),
            Either::Right((scenario, events)) => events
                .emit_scenario_events(feature, None, scenario, writer)
                .await
                .map(Either::Right),
        }
    }
}

/// Queue of all [`RuleOrScenario`] events.
#[derive(Debug)]
struct RulesAndScenariosQueue<World>(
    LinkedHashMap<RuleOrScenario, RuleOrScenarioEvents<World>>,
);

/// Either a [`gherkin::Rule`] or a [`gherkin::Scenario`].
type RuleOrScenario = Either<Arc<gherkin::Rule>, Arc<gherkin::Scenario>>;

/// Either a [`gherkin::Rule`] or a [`gherkin::Scenario`].
type RuleOrScenarioEvents<World> =
    Either<RuleEvents<World>, ScenarioEvents<World>>;

type NextRuleOrScenario<'events, World> = Either<
    (Arc<gherkin::Rule>, &'events mut RuleEvents<World>),
    (Arc<gherkin::Scenario>, &'events mut ScenarioEvents<World>),
>;

impl<World> RulesAndScenariosQueue<World> {
    /// Creates new [`RulesAndScenarios`].
    fn new() -> Self {
        RulesAndScenariosQueue(LinkedHashMap::new())
    }

    /// Inserts new [`Rule`].
    ///
    /// [`Rule`]: gherkin::Rule
    fn new_rule(&mut self, rule: Arc<gherkin::Rule>) {
        drop(
            self.0
                .insert(Either::Left(rule), Either::Left(RuleEvents::new())),
        );
    }

    /// Marks [`Rule`] as finished on [`Rule::Finished`].
    ///
    /// [`Rule`]: gherkin::Rule
    /// [`Rule::Finished`]: event::Rule::Finished
    fn rule_finished(&mut self, rule: Arc<gherkin::Rule>) {
        match self.0.get_mut(&Either::Left(rule)).unwrap() {
            Either::Left(ev) => {
                ev.finished = true;
            }
            Either::Right(_) => unreachable!(),
        }
    }

    /// Inserts new [`Scenario`] event.
    ///
    /// [`Scenario`]: gherkin::Scenario
    fn insert_scenario_event(
        &mut self,
        rule: Option<Arc<gherkin::Rule>>,
        scenario: Arc<gherkin::Scenario>,
        ev: event::Scenario<World>,
    ) {
        if let Some(rule) = rule {
            match self
                .0
                .get_mut(&Either::Left(rule.clone()))
                .unwrap_or_else(|| panic!("No Rule {}", rule.name))
            {
                Either::Left(rules) => rules
                    .scenarios
                    .entry(scenario)
                    .or_insert_with(ScenarioEvents::new)
                    .0
                    .push(ev),
                Either::Right(_) => unreachable!(),
            }
        } else {
            match self
                .0
                .entry(Either::Right(scenario))
                .or_insert_with(|| Either::Right(ScenarioEvents::new()))
            {
                Either::Right(events) => events.0.push(ev),
                Either::Left(_) => unreachable!(),
            }
        }
    }

    /// Returns currently outputted [`RuleOrScenario`].
    fn next_rule_or_scenario(
        &mut self,
    ) -> Option<NextRuleOrScenario<'_, World>> {
        Some(match self.0.iter_mut().next()? {
            (Either::Left(rule), Either::Left(events)) => {
                Either::Left((rule.clone(), events))
            }
            (Either::Right(scenario), Either::Right(events)) => {
                Either::Right((scenario.clone(), events))
            }
            _ => unreachable!(),
        })
    }
}

/// Queue of all events of a single [`Rule`].
///
/// [`Rule`]: gherkin::Rule
#[derive(Debug)]
struct RuleEvents<World> {
    started_emitted: bool,
    scenarios: LinkedHashMap<Arc<gherkin::Scenario>, ScenarioEvents<World>>,
    finished: bool,
}

impl<World> RuleEvents<World> {
    /// Creates new [`RuleEvents`].
    fn new() -> Self {
        Self {
            started_emitted: false,
            scenarios: LinkedHashMap::new(),
            finished: false,
        }
    }

    /// Returns currently outputted [`Scenario`].
    ///
    /// [`Scenario`]: gherkin::Scenario
    fn next_scenario(
        &mut self,
    ) -> Option<(Arc<gherkin::Scenario>, &mut ScenarioEvents<World>)> {
        self.scenarios
            .iter_mut()
            .next()
            .map(|(sc, ev)| (sc.clone(), ev))
    }

    /// Emits all ready [`Rule`] events. If some [`Rule`] was fully outputted,
    /// returns it. After that it should be removed.
    ///
    /// [`Rule`]: gherkin::Rule
    async fn emit_rule_events<Wr: Writer<World>>(
        &mut self,
        feature: Arc<gherkin::Feature>,
        rule: Arc<gherkin::Rule>,
        writer: &mut Wr,
    ) -> Option<Arc<gherkin::Rule>> {
        if !self.started_emitted {
            writer
                .handle_event(event::Cucumber::rule_started(
                    feature.clone(),
                    rule.clone(),
                ))
                .await;
            self.started_emitted = true;
        }

        while let Some((scenario, events)) = self.next_scenario() {
            if let Some(should_be_removed) = events
                .emit_scenario_events(
                    feature.clone(),
                    Some(rule.clone()),
                    scenario,
                    writer,
                )
                .await
            {
                drop(self.scenarios.remove(&should_be_removed));
            } else {
                break;
            }
        }

        if self.finished {
            writer
                .handle_event(event::Cucumber::rule_finished(
                    feature,
                    rule.clone(),
                ))
                .await;
            return Some(rule);
        }

        None
    }
}

/// Storage for [`Scenario`] events.
///
/// [`Scenario`]: gherkin::Scenario
#[derive(Debug)]
struct ScenarioEvents<World>(Vec<event::Scenario<World>>);

impl<World> ScenarioEvents<World> {
    /// Creates new [`ScenarioEvents`].
    fn new() -> Self {
        Self(Vec::new())
    }

    /// Emits all ready [`Scenario`] events. If some [`Scenario`] was fully
    /// outputted, returns it. After that it should be removed.
    ///
    /// [`Scenario`]: gherkin::Scenario
    async fn emit_scenario_events<Wr: Writer<World>>(
        &mut self,
        feature: Arc<gherkin::Feature>,
        rule: Option<Arc<gherkin::Rule>>,
        scenario: Arc<gherkin::Scenario>,
        writer: &mut Wr,
    ) -> Option<Arc<gherkin::Scenario>> {
        while !self.0.is_empty() {
            let ev = self.0.remove(0);
            let should_be_removed = matches!(ev, event::Scenario::Finished);

            let ev = event::Cucumber::scenario(
                feature.clone(),
                rule.clone(),
                scenario.clone(),
                ev,
            );
            writer.handle_event(ev).await;

            if should_be_removed {
                return Some(scenario.clone());
            }
        }
        None
    }
}
