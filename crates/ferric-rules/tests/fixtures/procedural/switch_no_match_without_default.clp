
(defrule probe =>
 (switch blue (case red then (printout t wrong crlf)))
 (printout t after crlf))
