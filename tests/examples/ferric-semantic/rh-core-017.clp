; RH-CORE-017: export-all exposes typed template values across a module boundary.
(defmodule STORE (export ?ALL))
(deftemplate STORE::entry (slot label (type STRING)) (slot value (type INTEGER)))
(deffacts STORE::seed (entry (label "alpha") (value 42)))
(defmodule VIEW (import STORE ?ALL))
(deftemplate VIEW::shown (slot value (type INTEGER)))
(defrule VIEW::show (entry (label ?s) (value ?n)) => (printout t ?s "=" ?n crlf) (assert (shown (value ?n))))
(defrule MAIN::start => (focus VIEW))
