(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show alias (member (create$ b c) (create$ a b c)))
)
