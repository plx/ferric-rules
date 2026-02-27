(deftemplate where
   (multislot x (type SYMBOL)))
(defrule yak ; This should be OK
   (where (x $?pds&:(member$ x ?pds)))
   =>)
