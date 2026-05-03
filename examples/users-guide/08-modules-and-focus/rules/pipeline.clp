;;; Two modules look at the same global facts. Focus controls which
;;; module's rules are eligible to fire at any moment.

(defmodule SENSORS (export deftemplate reading))

(deftemplate reading (slot kind) (slot value))

(defrule SENSORS::observe
    (reading (kind ?k) (value ?v))
    =>
    (printout t "SENSORS observed " ?k "=" ?v crlf))

(defmodule ALERTS (import SENSORS deftemplate ?ALL))

(defrule ALERTS::warn
    (reading (kind ?k) (value ?v))
    (test (> ?v 100))
    =>
    (printout t "ALERTS: high " ?k " (" ?v ")" crlf))
