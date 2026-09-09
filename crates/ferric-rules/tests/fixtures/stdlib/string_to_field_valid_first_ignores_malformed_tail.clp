(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show integer (string-to-field "42 \"unterminated"))
 (show float (string-to-field "2.5 ?*broken"))
 (show symbol (string-to-field "red ) ("))
 (show string (string-to-field "\"two words\" \"unterminated"))
)
