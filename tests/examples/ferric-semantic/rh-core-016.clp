; RH-CORE-016: explicit named template export is available to an importing focused module.
(defmodule DATA (export deftemplate task))
(deftemplate DATA::task (slot id (type SYMBOL)))
(deffacts DATA::seed (task (id a)))
(defmodule WORK (import DATA deftemplate task))
(deftemplate WORK::completed (slot id))
(defrule WORK::consume (task (id ?x)) => (printout t "work " ?x crlf) (assert (completed (id ?x))))
(defrule MAIN::start => (focus WORK))
