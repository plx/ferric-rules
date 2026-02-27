(deftemplate congestion            ; DR0445
   (field no-of-nets))
(deftemplate total                 ; DR0445
   (field net-name)
   (field cong))
(deffacts start                    ; DR0445
   (congestion (no-of-nets 5))
   (total (net-name 8) (cong nil))
   (total (net-name 4) (cong 5)))
(defrule p403                      ; DR0445
   ?t1 <-  (total (cong nil))
   (congestion (no-of-nets ?non))
   =>
   (retract ?t1))
(defrule p410                      ; DR0445
   (total (net-name ?nn) (cong ?non))
   (not (total (cong nil)))
   ?t <- (total (net-name ~?nn) (cong ?x&:(<= ?x ?non)))
   =>
   (retract ?t))
