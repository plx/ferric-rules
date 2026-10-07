(defmodule MAIN (export ?ALL))
(deftemplate go (slot x))
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::alarm (declare (auto-focus TRUE)) (go) => (printout t wrong crlf))
(defrule MAIN::start =>
  (bind ?f (assert (go (x 1)))) (printout t "assert:" (get-focus-stack) crlf)
  (retract ?f) (printout t "retract:" (get-focus-stack) crlf))
