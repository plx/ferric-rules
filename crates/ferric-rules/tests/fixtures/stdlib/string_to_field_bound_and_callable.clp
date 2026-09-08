(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(deffunction parse-field (?text) (string-to-field ?text))
(deffacts input (text "42 trailing"))
(defrule probe (text ?text) =>
 (show bound (string-to-field ?text))
 (show callable (parse-field "2.5 trailing"))
 (show quoted (parse-field "\"two words\" tail")))
