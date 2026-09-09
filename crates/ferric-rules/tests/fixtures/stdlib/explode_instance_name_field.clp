(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show instance (explode$ "[widget] a"))
 (printout t "instance-name:" (instance-namep (nth$ 1 (explode$ "[widget]"))) crlf)
)
