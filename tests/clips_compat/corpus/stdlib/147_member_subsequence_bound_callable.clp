(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(deffunction locate (?needle ?haystack) (member$ ?needle ?haystack))
(defgeneric locate-method)
(defmethod locate-method ((?needle MULTIFIELD) (?haystack MULTIFIELD))
 (member$ ?needle ?haystack))
(deffacts input (hay a b c d))
(defrule probe (hay $?haystack) =>
 (show bound (member$ (create$ b c) ?haystack))
 (show callable (locate (create$ b c) ?haystack))
 (show method (locate-method (create$ b c) ?haystack)))
