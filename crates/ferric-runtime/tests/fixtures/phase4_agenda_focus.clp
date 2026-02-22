;; Phase 4: Agenda and focus query function surface fixture.
;; Exercises multi-module focus stack and agenda visibility.

(defrule MAIN::init
    (go)
    =>
    (printout t "main-focus: " (get-focus) crlf)
    (assert (SENSOR::reading 42))
    (focus SENSOR)
    (printout t "after-focus-cmd: " (get-focus) crlf))

(defmodule SENSOR (export ?ALL))
(defrule SENSOR::process
    (reading ?v)
    =>
    (printout t "sensor-got: " ?v crlf)
    (printout t "sensor-focus: " (get-focus) crlf))

(deffacts MAIN::startup (go))
