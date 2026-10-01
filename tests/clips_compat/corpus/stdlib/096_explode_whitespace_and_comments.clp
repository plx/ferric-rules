(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show comments (explode$ "a;comment
\"two words\" ; trailing
3"))
 (show formfeed (explode$ "ab 3"))
 (show nbsp (explode$ "a b tail"))
 (show vertical-tab (explode$ "ab 3"))
 (show comment-only (explode$ "; ignored"))
 (show leading-control (explode$ "42 tail"))
)
