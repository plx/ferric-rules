(deftemplate pose-record
  (slot name (type SYMBOL))
  (slot x (type FLOAT))
  (slot y (type FLOAT))
  (slot reached-at (type FLOAT))
)

(deftemplate pose-order
  (slot curr (type SYMBOL))
  (slot next (type SYMBOL))
)

(defglobal
 ?*MAX-VEL-LIN* = 2.0
 ?*MAX-VEL-ANG* = 1.0
)

(deffacts poses
(pose-record (name bottom-left) (x 1.0) (y 1.0))
(pose-record (name bottom-right) (x 9.0) (y 1.0))
(pose-record (name top-right) (x 9.0) (y 9.0))
(pose-record (name top-left) (x 1.0) (y 9.0))
(pose-order (curr bottom-left) (next bottom-right))
(pose-order (curr bottom-right) (next top-right))
(pose-order (curr top-right) (next top-left))
(pose-order (curr top-left) (next bottom-left))
)

(defrule tb-pub-sub-init
" Create publisher for ros_cx_out."
  (not (ros-msgs-publisher (topic "/turtle1/cmd_vel")))
  (not (ros-msgs-subscription (topic "/turtle1/pose")))
  (not (executive-finalize))
=>
  (ros-msgs-create-publisher "/turtle1/cmd_vel" "geometry_msgs/msg/Twist")
  (ros-msgs-create-subscription "/turtle1/pose" "turtlesim/msg/Pose")
  (assert (curr-pose bottom-left))
)

(defrule tb-pose-no-vel-update-move
  (ros-msgs-subscription (topic ?sub))
  ?msg-f <- (ros-msgs-message (topic ?sub) (msg-ptr ?inc-msg))
  (test (and (= 0 (ros-msgs-get-field ?inc-msg "linear_velocity"))
             (= 0 (ros-msgs-get-field ?inc-msg "angular_velocity"))
  ))
  ?cp <- (curr-pose ?curr-pose)
  ?pr <- (pose-record (name ?curr-pose) (x ?x) (y ?y))
  (pose-order (curr ?curr-pose) (next ?next-pose))
  (ros-msgs-publisher (topic "/turtle1/cmd_vel"))
  =>
  (bind ?pos-x (ros-msgs-get-field ?inc-msg "x"))
  (bind ?pos-y (ros-msgs-get-field ?inc-msg "y"))
  (printout yellow "Target: ("?x "," ?y ") Curr: (" ?pos-x ","?pos-y")" crlf)
  (bind ?theta (ros-msgs-get-field ?inc-msg "theta"))
  (bind ?theta (+ ?theta (pi)))
  (if (and (< (abs (- ?pos-x ?x)) 0.1) (< (abs (- ?pos-y ?y)) 0.1)) then
    (printout blue "Goal reached" crlf)
    (modify ?pr (reached-at (now)))
    (retract ?cp)
    (assert (curr-pose ?next-pose))
   else
    (bind ?angle (atan2 (- ?pos-y ?y) (- ?pos-x ?x)))
	(printout magenta "current rot " ?theta " angle of target " ?angle crlf) 
    (bind ?angle-diff (- ?angle ?theta))
	(if (> ?angle-diff  (pi)) then (bind ?angle-diff (- ?angle-diff (* 2.0 (pi)))))
	(if (< ?angle-diff (* -1.0 (pi))) then (bind ?angle-diff (+ ?angle-diff (* 2.0 (pi)))))
    (if (> (abs ?angle-diff) 0.01) then
      ;(printout yellow "Angle-diff: " ?angle-diff crlf)
      (bind ?angle-diff (min ?angle-diff ?*MAX-VEL-ANG*))
      (bind ?angle-diff (max ?angle-diff (* -1.0 ?*MAX-VEL-ANG*)))
      (bind ?cmd-msg (ros-msgs-create-message "geometry_msgs/msg/Twist"))
      (bind ?angular-msg (ros-msgs-create-message "geometry_msgs/msg/Vector3"))
      (ros-msgs-set-field ?angular-msg "z" ?angle-diff)
      (ros-msgs-set-field ?cmd-msg "angular" ?angular-msg)
      (ros-msgs-publish ?cmd-msg "/turtle1/cmd_vel")
      (ros-msgs-destroy-message ?cmd-msg)
      (ros-msgs-destroy-message ?angular-msg)
     else
      (bind ?dist (sqrt (+ (* (- ?pos-x ?x) (- ?pos-x ?x))
                           (* (- ?pos-y ?y) (- ?pos-y ?y))
      )))
      (bind ?dist (min ?dist ?*MAX-VEL-LIN*))
      ;(printout yellow "Distance: " ?dist crlf)
      (bind ?cmd-msg (ros-msgs-create-message "geometry_msgs/msg/Twist"))
      (bind ?linear-msg (ros-msgs-create-message "geometry_msgs/msg/Vector3"))
      (ros-msgs-set-field ?linear-msg "x" ?dist)
      (ros-msgs-set-field ?cmd-msg "linear" ?linear-msg)
      (ros-msgs-publish ?cmd-msg "/turtle1/cmd_vel")
      (ros-msgs-destroy-message ?cmd-msg)
      (ros-msgs-destroy-message ?linear-msg)
    )
  )
  (ros-msgs-destroy-message ?inc-msg)
  (retract ?msg-f)
)


(defrule tb-pose-vel-update-ignore
  (ros-msgs-subscription (topic ?sub))
  ?msg-f <- (ros-msgs-message (topic ?sub) (msg-ptr ?inc-msg))
  (test (or (<> 0 (ros-msgs-get-field ?inc-msg "linear_velocity"))
             (<> 0 (ros-msgs-get-field ?inc-msg "angular_velocity"))
  ))
  =>
  (ros-msgs-destroy-message ?inc-msg)
  (retract ?msg-f)
)

(defrule tb-pose-vel-update-ignore
  (declare (salience -1000))
  (ros-msgs-subscription (topic ?sub))
  ?msg-f <- (ros-msgs-message (topic ?sub) (msg-ptr ?inc-msg))
  =>
  (ros-msgs-destroy-message ?inc-msg)
  (retract ?msg-f)
)
