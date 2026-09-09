;; The callable's module can see its private template; the caller cannot.
(defmodule DATA (export deffunction ?ALL))
(deftemplate DATA::hidden (slot value))
(deffacts DATA::seed (hidden (value 10)))
(deffunction hidden-count () (length$ (find-all-facts ((?f hidden)) TRUE)))
(defmodule MAIN (import DATA deffunction ?ALL))
(defrule MAIN::probe => (printout t (hidden-count) crlf))
