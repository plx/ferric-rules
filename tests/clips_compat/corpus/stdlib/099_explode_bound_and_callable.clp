(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(deffunction fields (?text) (explode$ ?text))
(deffacts input (text "a \"two words\" 3"))
(defrule probe (text ?text) =>
 (show bound (explode$ ?text))
 (show callable (fields ?text)))
