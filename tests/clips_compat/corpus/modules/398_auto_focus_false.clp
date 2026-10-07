(defmodule MAIN (export ?ALL))
(deftemplate go)
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::alarm (declare (auto-focus FALSE)) (go) => (printout t watch crlf))
(defrule MAIN::start =>
  (assert (go)) (printout t (get-focus-stack) crlf) (focus WATCH))
