(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show adjacent (explode$ "a\"two words\"3 \"x\"\"y\""))
 (show quote (explode$ "\"a\\\"b\" \"a\\\\b\""))
 (show escapes (explode$ "\"a\\nb\" \"a\\tb\" \"a\\qb\""))
 (show newline (explode$ "\"a
b\" tail"))
)
