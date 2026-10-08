(defglobal ?*address* = FALSE)
(deffacts seed (item))
(defrule capture ?f <- (item) => (bind ?*address* ?f))
