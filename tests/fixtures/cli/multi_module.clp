(defmodule MAIN (export ?ALL))
(defmodule REPORT (import MAIN ?ALL))
(defrule MAIN::start (initial-fact) => (printout t "Starting" crlf))
