(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show numbers (explode$ "42 -17 +17 00042 2.5 2e3 .5 -.5 1. -0.0 9223372036854775807 -9223372036854775808"))
 (show symbols (explode$ "42abc 1.abc 1e 1e+ 1.2.3 + - NaN inf 0x10"))
)
