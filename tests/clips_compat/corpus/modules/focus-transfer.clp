;; A focus action enables the target module before MAIN resumes.
;; Level: interaction
;; Covers: modules, focus-transfer
(defmodule WORK)
(defrule WORK::work => (printout t "work" crlf))
(defrule MAIN::start (declare (salience 10)) => (printout t "start" crlf) (focus WORK))
(defrule MAIN::finish => (printout t "finish" crlf))
