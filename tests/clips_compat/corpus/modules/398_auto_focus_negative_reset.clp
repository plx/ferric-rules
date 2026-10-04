(defmodule MAIN (export ?ALL))
(deftemplate block)
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::absent (declare (auto-focus TRUE)) (not (block)) =>
  (printout t "absent:" (get-focus-stack) crlf))
(defrule MAIN::report => (printout t main crlf))
