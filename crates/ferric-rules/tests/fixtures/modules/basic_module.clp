; Basic module with export/import: cross-module template access.
; In Ferric, deftemplate is defined with an unqualified name in the module context.
(defmodule SENSORS (export deftemplate reading))
(deftemplate reading (slot sensor) (slot value))

(defmodule MAIN (import SENSORS deftemplate reading))

(deffacts MAIN::startup
    (reading (sensor temp) (value 72)))

(defrule MAIN::report
    (reading (sensor ?s) (value ?v))
    =>
    (printout t "Sensor " ?s " = " ?v crlf))
