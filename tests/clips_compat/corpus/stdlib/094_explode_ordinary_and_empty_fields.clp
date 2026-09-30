(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show ordinary (explode$ "a b 3 2.5 -17 +9"))
 (show empty (explode$ ""))
 (show spaces (explode$ " 	
 "))
 (show quotes (explode$ "\"\" \"two words\" \"3\" 3"))
)
