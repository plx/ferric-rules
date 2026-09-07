"""Tests for fact operations: assert, retract, get, find."""

import pytest
import ferric


class TestAssertString:
    def test_assert_ordered_fact(self, engine):
        ids = engine.assert_string("(color red)")
        assert isinstance(ids, list)
        assert len(ids) == 1
        assert isinstance(ids[0], int)
        assert ids[0] > 0

    def test_assert_multiple_in_one_call(self, engine):
        ids = engine.assert_string("(color red) (color blue)")
        assert len(ids) == 2
        assert ids[0] != ids[1]

    def test_assert_separate_calls(self, engine):
        ids1 = engine.assert_string("(color red)")
        ids2 = engine.assert_string("(color blue)")
        assert ids1[0] != ids2[0]

    def test_assert_string_with_number(self, engine):
        ids = engine.assert_string("(count 42)")
        fact = engine.get_fact(ids[0])
        assert fact is not None
        assert fact.relation == "count"

    def test_assert_string_empty_raises(self, engine):
        """Malformed or empty input that produces no facts should error."""
        with pytest.raises(ferric.FerricError):
            engine.assert_string("")


class TestAssertFact:
    def test_assert_structured(self, engine):
        fid = engine.assert_fact("color", "red")
        assert isinstance(fid, int)
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.relation == "color"
        assert fact.fact_type == ferric.FactType.ORDERED

    def test_assert_with_int(self, engine):
        fid = engine.assert_fact("count", 42)
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.fields[0] == 42

    def test_assert_with_float(self, engine):
        fid = engine.assert_fact("temperature", 98.6)
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.fields[0] == pytest.approx(98.6)

    def test_assert_multiple_fields(self, engine):
        fid = engine.assert_fact("point", 3, 4.5)
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.fields[0] == 3
        assert fact.fields[1] == pytest.approx(4.5)


class TestRetract:
    def test_retract_existing(self, engine):
        fid = engine.assert_fact("color", "red")
        engine.retract(fid)
        assert engine.get_fact(fid) is None

    def test_retract_nonexistent_raises(self, engine):
        fid = engine.assert_fact("color", "red")
        engine.retract(fid)
        with pytest.raises(ferric.FerricFactNotFoundError):
            engine.retract(fid)


class TestGetFact:
    def test_get_existing(self, engine):
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        assert fact is not None
        assert fact.id == fid

    def test_get_missing_returns_none(self, engine):
        fid = engine.assert_fact("color", "red")
        engine.retract(fid)
        assert engine.get_fact(fid) is None

    def test_fact_exposes_engine_id(self, engine):
        # engine_id backs cross-engine fact equality/hashing; ensure it is
        # populated and stable across snapshots of the same fact.
        fid = engine.assert_fact("color", "red")
        fact = engine.get_fact(fid)
        assert fact.engine_id > 0
        assert engine.get_fact(fid).engine_id == fact.engine_id


class TestFacts:
    def test_facts_empty(self, engine):
        facts = engine.facts()
        assert facts == []

    def test_facts_returns_all(self, engine):
        engine.assert_fact("color", "red")
        engine.assert_fact("color", "blue")
        facts = engine.facts()
        assert len(facts) == 2

    def test_facts_are_fact_objects(self, engine):
        engine.assert_fact("color", "red")
        facts = engine.facts()
        assert len(facts) == 1
        assert isinstance(facts[0], ferric.Fact)


class TestFindFacts:
    def test_find_by_relation(self, engine):
        c1 = engine.assert_fact("color", "red")
        c2 = engine.assert_fact("color", "blue")
        s1 = engine.assert_fact("shape", "circle")
        # find_facts must return exactly the facts of that relation (by id),
        # not merely the right count.
        assert {f.id for f in engine.find_facts("color")} == {c1, c2}
        assert {f.id for f in engine.find_facts("shape")} == {s1}

    def test_find_no_match(self, engine):
        engine.assert_fact("color", "red")
        found = engine.find_facts("nonexistent")
        assert found == []


class TestHostProvenance:
    def test_foreign_fact_id_cannot_alias_a_local_fact(self):
        with ferric.Engine() as first, ferric.Engine() as second:
            first_id = first.assert_fact("item", 1)
            second_id = second.assert_fact("item", 2)
            assert first_id != second_id
            assert first_id >= 1 << 63
            assert second_id >= 1 << 63
            assert second.get_fact(first_id) is None
            assert first_id not in second
            with pytest.raises(ferric.FerricFactNotFoundError):
                second.retract(first_id)
            assert second.get_fact(second_id).fields == [2]
            second.retract(second_id)
            assert second.get_fact(second_id) is None

    def test_reset_and_restore_invalidate_transient_ids(self):
        with ferric.Engine.from_source("(deffacts seed (item 7))") as engine:
            engine.reset()
            old_fact = engine.find_facts("item")[0]
            snapshot = engine.serialize()
            engine.reset()
            new_fact = engine.find_facts("item")[0]
            assert new_fact.id != old_fact.id
            assert engine.get_fact(old_fact.id) is None
            with pytest.raises(ferric.FerricFactNotFoundError):
                engine.retract(old_fact.id)
            with ferric.Engine.from_snapshot(snapshot) as restored:
                restored_fact = restored.find_facts("item")[0]
                assert restored_fact.id not in {old_fact.id, new_fact.id}
                assert restored.get_fact(old_fact.id) is None
                assert restored_fact.fields == [7]
        assert old_fact.fields == [7]

    def test_owned_values_reintern_after_source_engine_closes(self):
        with ferric.Engine() as source:
            source.assert_fact("unrelated", ferric.Symbol("first"))
            fact_id = source.assert_fact(
                "item", ferric.Symbol("ready"),
                [ferric.Symbol("nested"), ferric.String("text")],
            )
            owned_fact = source.get_fact(fact_id)
        with ferric.Engine.from_source(
            '(defrule selected (item ready ?payload) => (printout t "selected" crlf))'
        ) as target:
            target.assert_fact("different", ferric.Symbol("intern-order"))
            copy_id = target.assert_fact(owned_fact.relation, *owned_fact.fields)
            assert target.get_fact(copy_id).fields == owned_fact.fields
            assert target.run().rules_fired == 1
            assert target.get_output("t") == "selected\n"
