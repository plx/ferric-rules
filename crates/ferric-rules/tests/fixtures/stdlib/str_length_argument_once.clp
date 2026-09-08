(defglobal ?*calls* = 0)
(deffunction next-word () (bind ?*calls* (+ ?*calls* 1)) abc)
(defrule probe => (printout t (str-length (next-word)) ":" ?*calls* crlf))
