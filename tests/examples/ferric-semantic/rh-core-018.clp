; RH-CORE-018: focus arguments establish deterministic module execution order.
(defmodule ONE)
(defmodule TWO)
(defrule ONE::first => (printout t "one" crlf))
(defrule TWO::second => (printout t "two" crlf))
(defrule MAIN::start => (printout t "start" crlf) (focus ONE TWO))
