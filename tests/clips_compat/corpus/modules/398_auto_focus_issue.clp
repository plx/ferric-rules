(defmodule MAIN (export ?ALL))
(deftemplate go (slot x))
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::alarm (declare (auto-focus TRUE)) (go (x ?x)) => (printout t "alarm " ?x crlf))
(defrule MAIN::main-rule (go (x ?x&:(< ?x 10))) =>
  (printout t "main " ?x crlf) (assert (go (x (+ ?x 10)))))
(deffacts MAIN::f (go (x 1)))
