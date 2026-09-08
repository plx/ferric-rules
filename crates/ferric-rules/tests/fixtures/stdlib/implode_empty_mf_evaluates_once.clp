(defglobal ?*trace* = 0)
(deffunction empty-value () (bind ?*trace* (+ ?*trace* 1)) (create$))
(defrule probe => (printout t "[" (implode$ (empty-value)) "]:" ?*trace* crlf))
