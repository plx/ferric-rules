(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show vertical-tab (string-to-field "42 tail"))
 (show form-feed (string-to-field "2.5 tail"))
 (show mixed (string-to-field " 	
 red tail"))
)
