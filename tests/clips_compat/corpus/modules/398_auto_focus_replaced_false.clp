(defmodule MAIN (export ?ALL))
(deftemplate go)
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::alarm (declare (auto-focus TRUE)) (go) => (printout t wrong crlf))
(defrule WATCH::alarm (declare (auto-focus FALSE)) (go) => (printout t replaced crlf))
(defrule MAIN::start =>
  (assert (go)) (printout t (get-focus-stack) crlf) (focus WATCH))
