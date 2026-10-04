(defmodule MAIN (export ?ALL))
(deftemplate block)
(deftemplate probe)
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::absent (declare (auto-focus TRUE)) (not (block)) => (printout t wrong crlf))
(defrule WATCH::visible (probe) => (printout t retained crlf))
(defrule MAIN::report => (printout t main crlf))
;; Reset must first create and then cancel the auto-focus activation.
;; Loading these rules after reset has different focus history.
(deffacts MAIN::seed (block) (probe))
