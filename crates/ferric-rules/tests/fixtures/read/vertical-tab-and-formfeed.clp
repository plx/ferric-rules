(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show vertical-tab (read))
 (show formfeed (read))
 (show inner (read))
 (show after (read))
)
