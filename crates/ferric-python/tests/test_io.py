"""Tests for I/O: get_output, clear_output, push_input."""

import ferric


class TestGetOutput:
    def test_no_output(self, engine):
        assert engine.get_output("t") is None

    def test_printout_captured(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (go))
            (defrule print-hello
                (go)
                =>
                (printout t "hello world" crlf))
        """)
        engine.run()
        output = engine.get_output("t")
        assert output is not None
        assert "hello world" in output


class TestClearOutput:
    def test_clear_output(self):
        engine = ferric.Engine.from_source("""
            (deffacts startup (go))
            (defrule print-hello
                (go)
                =>
                (printout t "hello" crlf))
        """)
        engine.run()
        assert engine.get_output("t") is not None
        engine.clear_output("t")
        assert engine.get_output("t") is None


class TestPushInput:
    def test_push_input(self, engine):
        # Just verify push_input doesn't error
        engine.push_input("test line")


class TestDiagnostics:
    def test_diagnostics_empty(self, engine):
        assert engine.diagnostics == []
