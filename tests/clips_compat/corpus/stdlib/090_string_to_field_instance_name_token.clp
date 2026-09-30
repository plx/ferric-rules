(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show instance (string-to-field "[widget] rest"))
 (printout t "instance-name:" (instance-namep (string-to-field "[widget] rest")) crlf))
