(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show scalar (member$ b (create$ a b c)))
 (show singleton (member$ (create$ b) (create$ a b c)))
 (show head (member$ (create$ a b) (create$ a b c)))
 (show tail (member$ (create$ b c) (create$ a b c)))
 (show whole (member$ (create$ a b c) (create$ a b c)))
 (show longer (member$ (create$ a b c d) (create$ a b c)))
 (show gapped (member$ (create$ a c) (create$ a b c)))
)
