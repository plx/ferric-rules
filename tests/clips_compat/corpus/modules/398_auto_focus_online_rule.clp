(defmodule MAIN (export ?ALL))
(deftemplate go (slot x))
(defmodule WATCH (import MAIN ?ALL))
(defrule MAIN::start =>
  (assert (go (x 7)))
  (printout t (build "(defrule WATCH::online (declare (auto-focus TRUE)) (go (x ?x)) => (printout t watch ?x crlf))")
    ":" (get-focus-stack) crlf))
