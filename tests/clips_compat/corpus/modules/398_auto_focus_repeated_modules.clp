(defmodule MAIN (export ?ALL))
(deftemplate go (slot x))
(defmodule A (import MAIN ?ALL))
(defrule A::r (declare (auto-focus TRUE)) (go (x ?x)) => (printout t A ?x crlf))
(defmodule B (import MAIN ?ALL))
(defrule B::r (declare (auto-focus TRUE)) (go (x ?x)) => (printout t B ?x crlf))
(defrule MAIN::start =>
  (assert (go (x 1))) (printout t "first:" (get-focus-stack) crlf)
  (assert (go (x 2))) (printout t "second:" (get-focus-stack) crlf))
