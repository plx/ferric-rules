"""Tests for structured template fact operations: assert_template, slot access."""

import pytest
import ferric


PERSON_TEMPLATE = """
(deftemplate person
    (slot name (type STRING))
    (slot age (type INTEGER) (default 0))
    (slot active (type SYMBOL) (default TRUE)))
"""


@pytest.fixture
def template_engine():
    """An engine with a person template loaded and reset."""
    return ferric.Engine.from_source(PERSON_TEMPLATE)


class TestAssertTemplate:
    def test_basic(self, template_engine):
        fid = template_engine.assert_template("person", name="Alice", age=30)
        assert isinstance(fid, int)
        assert fid > 0

    def test_get_fact_is_template(self, template_engine):
        fid = template_engine.assert_template("person", name="Alice", age=30)
        fact = template_engine.get_fact(fid)
        assert fact is not None
        assert fact.fact_type == ferric.FactType.TEMPLATE
        assert fact.template_name == "person"

    def test_slots_dict(self, template_engine):
        fid = template_engine.assert_template("person", name="Bob", age=25)
        fact = template_engine.get_fact(fid)
        assert fact.slots is not None
        assert fact.slots["name"] == "Bob"
        assert fact.slots["age"] == 25

    def test_defaults_filled(self, template_engine):
        """Unspecified slots should get their declared defaults."""
        fid = template_engine.assert_template("person", name="Charlie")
        fact = template_engine.get_fact(fid)
        assert fact.slots["name"] == "Charlie"
        assert fact.slots["age"] == 0  # default
        assert fact.slots["active"] == "TRUE"  # default

    def test_all_defaults(self, template_engine):
        """No kwargs → all slots get defaults."""
        fid = template_engine.assert_template("person")
        fact = template_engine.get_fact(fid)
        assert fact.slots["age"] == 0
        assert fact.slots["active"] == "TRUE"

    def test_override_default(self, template_engine):
        fid = template_engine.assert_template("person", name="Dave", active="FALSE")
        fact = template_engine.get_fact(fid)
        assert fact.slots["active"] == "FALSE"

    def test_template_not_found(self, template_engine):
        with pytest.raises(ferric.FerricTemplateNotFoundError):
            template_engine.assert_template("nonexistent", name="X")

    def test_slot_not_found(self, template_engine):
        with pytest.raises(ferric.FerricSlotNotFoundError):
            template_engine.assert_template("person", bad_slot="X")

    def test_template_facts_in_facts_list(self, template_engine):
        template_engine.assert_template("person", name="Alice")
        template_engine.assert_template("person", name="Bob")
        # Template facts show up in the full facts list.
        all_facts = template_engine.facts()
        person_facts = [f for f in all_facts if f.template_name == "person"]
        assert len(person_facts) == 2

    def test_retract_template_fact(self, template_engine):
        fid = template_engine.assert_template("person", name="Alice")
        template_engine.retract(fid)
        assert template_engine.get_fact(fid) is None

    def test_multiple_templates(self):
        source = """
        (deftemplate person (slot name))
        (deftemplate car (slot make) (slot model))
        """
        engine = ferric.Engine.from_source(source)
        p = engine.assert_template("person", name="Alice")
        c = engine.assert_template("car", make="Toyota", model="Camry")
        assert p != c
        pf = engine.get_fact(p)
        cf = engine.get_fact(c)
        assert pf.template_name == "person"
        assert cf.template_name == "car"
        assert cf.slots["make"] == "Toyota"
        assert cf.slots["model"] == "Camry"


class TestTemplateFactWithRules:
    def test_rule_fires_on_template_fact(self):
        source = """
        (deftemplate person (slot name) (slot age (type INTEGER) (default 0)))
        (defrule greet-adult
            (person (name ?n) (age ?a&:(> ?a 18)))
            =>
            (assert (adult ?n)))
        """
        engine = ferric.Engine.from_source(source)
        engine.assert_template("person", name="Alice", age=30)
        result = engine.run()
        assert result.rules_fired == 1
        adults = engine.find_facts("adult")
        assert len(adults) == 1

    def test_rule_does_not_fire_below_threshold(self):
        source = """
        (deftemplate person (slot name) (slot age (type INTEGER) (default 0)))
        (defrule greet-adult
            (person (name ?n) (age ?a&:(> ?a 18)))
            =>
            (assert (adult ?n)))
        """
        engine = ferric.Engine.from_source(source)
        engine.assert_template("person", name="Bob", age=10)
        result = engine.run()
        assert result.rules_fired == 0
